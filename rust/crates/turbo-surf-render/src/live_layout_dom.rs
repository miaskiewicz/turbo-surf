//! Host-only live tree and CSS snapshot access. Included under the binding adapter
//! so native handles and private document ownership never escape to page JS.
use super::*;
use crate::live_layout::{capture_composed_tree, LiveTreeInput};
use crate::live_layout_composed::Composition;
use crate::live_stylesheets::SheetSnapshot;
use std::collections::HashSet;

pub(crate) fn retire_context(_scope: &mut v8::PinScope, context: v8::Local<v8::Context>) {
    crate::browser_env::retire_custom_element_context(context);
    let Some(id) = context.get_slot::<DomContextId>() else {
        return;
    };
    let _dom = DomScope::enter(id.0);
    DOM.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.browsing_context = false;
        }
    });
}

pub(crate) struct NativeLayoutInput<'s> {
    pub(crate) owner_context: Option<v8::Local<'s, v8::Context>>,
    pub(crate) tree: LiveTreeInput,
    pub(crate) handle: Handle,
    pub(crate) connected: bool,
    pub(crate) browsing_context: bool,
    pub(crate) sheets: Vec<SheetSnapshot>,
    pub(crate) range: Option<Vec<crate::live_layout_range::Selection>>,
}

pub(crate) fn capture<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<NativeLayoutInput<'s>, String> {
    if let Some(handle) = handle_of(scope, receiver) {
        let _dom = DomScope::for_object(scope, receiver);
        let text = with_tree(|tree| tree.node_type_id(handle) == 3).unwrap_or(false);
        capture_kind(scope, receiver, if text { 0 } else { 1 })
    } else {
        capture_range(scope, receiver)
    }
}
pub(crate) fn capture_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<NativeLayoutInput<'s>, String> {
    capture_kind(scope, receiver, 9)
}
fn capture_kind<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    kind: u8,
) -> Result<NativeLayoutInput<'s>, String> {
    let _capture_profile =
        crate::layout_profile::Timer::new(scope, crate::layout_profile::Stage::Capture);
    let _dom = DomScope::for_object(scope, receiver);
    let handle = handle_of(scope, receiver).ok_or("Illegal invocation")?;
    let document_id = u32::try_from(DOM_ID.with(|id| id.get()))
        .map_err(|_| "native document identity exceeds layout source range")?;
    let browsing_context = DOM.with(|state| {
        state
            .borrow()
            .as_ref()
            .is_some_and(|state| state.browsing_context)
    });
    let composition_profile =
        crate::layout_profile::Timer::new(scope, crate::layout_profile::Stage::Composition);
    let (connected, root, mut graph, sheets) = with_tree(|tree| {
        let actual = tree.node_type_id(handle);
        let valid = match kind {
            0 => matches!(actual, 1 | 3 | 9 | 11),
            9 => actual == 9 || tree.is_shadow_root(handle),
            _ => actual == kind,
        };
        if !valid {
            return Err(String::from("Illegal invocation"));
        }
        let connected = is_connected(tree, handle);
        let graph = Composition::scopes(tree)?;
        let sheets = graph
            .scopes
            .iter()
            .filter_map(|(&node, &root)| {
                matches!(tree.local_name(node), Some("style" | "link")).then_some((node, root))
            })
            .collect::<HashSet<_>>();
        Ok((connected, tree.root(), graph, sheets))
    })
    .ok_or("native layout document is unavailable")??;
    if connected && browsing_context {
        let mut pending = vec![(root, 0usize)];
        while let Some((node, depth)) = pending.pop() {
            if depth > 256 || graph.children.len() >= 50_000 {
                return Err("native composed layout exceeds source budget".into());
            }
            if graph.children.contains_key(&node) {
                return Err("native composed layout repeats a source".into());
            }
            // The native light-tree children are already authoritative unless a
            // shadow host or an HTML slot needs private distribution records.
            if scope.is_execution_terminating() {
                return Err("native composition interrupted".into());
            }
            let native = with_tree(|tree| native_composed_children(tree, node))
                .ok_or("native layout document is unavailable")?;
            let handles = if let Some(children) = native {
                children
            } else {
                let wrapper = wrap(scope, node);
                let children = host_composed_children(scope, wrapper)
                    .ok_or("native composition interrupted")?;
                let mut handles = Vec::with_capacity(children.len());
                for child in children {
                    if dom_id_of(scope, child) != u64::from(document_id) {
                        return Err("native composition contains a foreign document source".into());
                    }
                    handles.push(
                        handle_of(scope, child)
                            .ok_or("native composition child source unavailable")?,
                    );
                }
                handles
            };
            for &child in &handles {
                if graph.parents.insert(child, node).is_some() {
                    return Err("native composed layout repeats a child".into());
                }
            }
            pending.extend(handles.iter().rev().map(|child| (*child, depth + 1)));
            graph.children.insert(node, handles);
        }
    }
    drop(composition_profile);
    graph.inline_overrides = css_policy::cascade_policy(scope)
        .inline_overrides
        .into_iter()
        .collect();
    // Ordinary documents use the same native matched cascade as shadow scopes.
    // Falling back here loses functional selector arguments and specificity.
    let composed = connected && browsing_context;
    if composed {
        graph.ua_rules =
            with_tree(|tree| crate::live_layout_scoped::ua_rules(tree, document_id, &graph))
                .ok_or("native UA owner tree unavailable")??;
    }
    let tree_profile =
        crate::layout_profile::Timer::new(scope, crate::layout_profile::Stage::TreeSnapshot);
    let tree = with_tree(|tree| {
        if connected && browsing_context {
            capture_composed_tree(tree, document_id, composed.then(|| graph.clone()))
        } else {
            Ok(LiveTreeInput {
                document_id,
                tree_version: tree.version,
                nodes: Vec::new(),
                identities: Vec::new(),
                svg_viewports: Vec::new(),
                original_text: std::collections::HashMap::new(),
                composition: None,
                quirks: false,
                body: None,
            })
        }
    })
    .ok_or("native layout document is unavailable")??;
    drop(tree_profile);
    let owner_context = DOM_CONTEXTS.with(|contexts| {
        contexts
            .borrow()
            .get(&DOM_ID.with(|id| id.get()))
            .and_then(|context| context.to_local(scope))
    });
    if !connected || !browsing_context {
        return Ok(NativeLayoutInput {
            owner_context,
            tree,
            handle,
            connected,
            browsing_context,
            sheets: Vec::new(),
            range: None,
        });
    }
    let _stylesheets_profile =
        crate::layout_profile::Timer::new(scope, crate::layout_profile::Stage::Stylesheets);
    let document = wrap(scope, root);
    let record = collection_records::get(scope, document)
        .and_then(|record| v8::Local::<v8::Object>::try_from(record).ok())
        .ok_or("native layout document interface is unavailable")?;
    let host = v8::Local::<v8::Object>::try_from(own(scope, record, "cssHost")?)
        .map_err(|_| "native layout CSS host is unavailable")?;
    let mut result = Vec::new();
    // Traverse the original scope trees for stylesheet order, including light
    // style elements excluded by slot placement. Their rules still style DOM.
    let mut roots = vec![root];
    roots.extend(graph.hosts.keys().copied());
    for scope_root in roots {
        let ordered = with_tree(|tree| ordered_stylesheets(tree, scope_root, &sheets))
            .ok_or("native CSS owner tree unavailable")?;
        for handle in ordered {
            let node = wrap(scope, handle);
            let value = call(scope, host, "sheet", &[node.into()])?;
            if !value.is_null_or_undefined() {
                let mut sheet = export(scope, host, value)?;
                prepare_sheet(scope, &mut sheet, document_id, scope_root, &graph)?;
                result.push(sheet);
            }
        }
        let owner = wrap(scope, scope_root);
        let adopted = call(scope, host, "readAdopted", &[owner.into()])?;
        let adopted = v8::Local::<v8::Array>::try_from(adopted)
            .map_err(|_| "native CSS adopted snapshot is not an array")?;
        for index in 0..adopted.length() {
            let sheet = adopted
                .get_index(scope, index)
                .ok_or("native CSS adopted read interrupted")?;
            let mut sheet = export(scope, host, sheet)?;
            prepare_sheet(scope, &mut sheet, document_id, scope_root, &graph)?;
            result.push(sheet);
        }
    }
    Ok(NativeLayoutInput {
        owner_context,
        tree,
        handle,
        connected,
        browsing_context,
        sheets: result,
        range: None,
    })
}
fn native_composed_children(tree: &Tree, node: Handle) -> Option<Vec<Handle>> {
    if tree.shadow_root(node).is_some()
        || tree.node_type_id(node) == 1
            && tree.namespace_uri(node) == Some("http://www.w3.org/1999/xhtml")
            && tree.local_name(node) == Some("slot")
    {
        return None;
    }
    Some(tree.children(node))
}
fn ordered_stylesheets(
    tree: &Tree,
    root: Handle,
    sheets: &HashSet<(Handle, Handle)>,
) -> Vec<Handle> {
    let mut ordered = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if sheets.contains(&(node, root)) {
            ordered.push(node);
        }
        pending.extend(tree.children(node).into_iter().rev());
    }
    ordered
}
fn prepare_sheet(
    scope: &v8::PinScope<'_, '_>,
    sheet: &mut SheetSnapshot,
    document: u32,
    root: Handle,
    graph: &Composition,
) -> Result<(), String> {
    let _profile =
        crate::layout_profile::Timer::new(scope, crate::layout_profile::Stage::SelectorPreparation);
    with_tree(|tree| crate::live_layout_scoped::prepare_sheet(tree, document, root, graph, sheet))
        .ok_or("native CSS owner tree unavailable")?
}
fn capture_range<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    range: v8::Local<'s, v8::Object>,
) -> Result<NativeLayoutInput<'s>, String> {
    let name = v8::String::new(scope, "turbo-dom.Range.record")
        .ok_or("native Range private key interrupted")?;
    let key = v8::Private::for_api(scope, Some(name));
    let record = range
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
        .ok_or("Illegal invocation")?;
    let (start, start_offset) = range_boundary(scope, record, "startContainer", "startOffset")?;
    let (end, end_offset) = range_boundary(scope, record, "endContainer", "endOffset")?;
    if dom_id_of(scope, start) != dom_id_of(scope, end) {
        return Err("native Range endpoints have different owner documents".into());
    }
    let mut input = capture_kind(scope, start, 0)?;
    let _dom = DomScope::for_object(scope, start);
    let start_handle = handle_of(scope, start).ok_or("native Range start source unavailable")?;
    let end_handle = handle_of(scope, end).ok_or("native Range end source unavailable")?;
    if !input.connected || !input.browsing_context {
        input.range = Some(Vec::new());
        return Ok(input);
    }
    input.range = Some(
        with_tree(|tree| {
            crate::live_layout_range::select(
                tree,
                input.tree.document_id,
                (start_handle, start_offset),
                (end_handle, end_offset),
            )
        })
        .ok_or("native Range owner tree unavailable")??,
    );
    Ok(input)
}
fn range_boundary<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    record: v8::Local<'s, v8::Object>,
    container: &str,
    offset: &str,
) -> Result<(v8::Local<'s, v8::Object>, usize), String> {
    let node = v8::Local::<v8::Object>::try_from(own(scope, record, container)?)
        .map_err(|_| "native Range boundary node unavailable")?;
    let value = own(scope, record, offset)?;
    if !value.is_uint32() {
        return Err("native Range boundary offset is invalid".into());
    }
    let offset = value
        .uint32_value(scope)
        .ok_or("native Range offset read interrupted")?;
    Ok((node, offset as usize))
}

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let key = v8::String::new(scope, name).ok_or("native layout key allocation interrupted")?;
    if object.has_own_property(scope, key.into()) != Some(true) {
        return Err(format!("native layout private field missing: {name}"));
    }
    object
        .get(scope, key.into())
        .ok_or("native layout private read interrupted".into())
}
fn call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host: v8::Local<'s, v8::Object>,
    name: &str,
    args: &[v8::Local<'s, v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, String> {
    let method = v8::Local::<v8::Function>::try_from(own(scope, host, name)?)
        .map_err(|_| "native CSS private method unavailable")?;
    method
        .call(scope, host.into(), args)
        .ok_or("native CSS private snapshot interrupted".into())
}
fn export<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host: v8::Local<'s, v8::Object>,
    sheet: v8::Local<'s, v8::Value>,
) -> Result<SheetSnapshot, String> {
    let _profile =
        crate::layout_profile::Timer::new(scope, crate::layout_profile::Stage::SheetExport);
    let record = call(scope, host, "exportSheet", &[sheet])?;
    deno_core::serde_v8::from_v8(scope, record)
        .map_err(|error| format!("native CSS snapshot conversion: {error}"))
}

pub(crate) fn wrap_elements<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    ids: &[turbo_dom_layout::layout_snapshot::SourceId],
    kind: u8,
) -> Result<v8::Local<'s, v8::Array>, String> {
    let _dom = DomScope::for_object(scope, document);
    let document_id = u32::try_from(DOM_ID.with(|id| id.get()))
        .map_err(|_| "native hit document identity exceeds range")?;
    let result = v8::Array::new(scope, 0);
    for (index, id) in ids.iter().enumerate() {
        if (id.0 >> 32) != u64::from(document_id) {
            return Err("native hit foreign document source".into());
        }
        let handle = Handle::from_raw(id.0 as u32);
        if with_tree(|tree| tree.node_type_id(handle) != kind || !is_connected(tree, handle))
            .unwrap_or(true)
        {
            return Err("native hit detached or wrong node kind source".into());
        }
        let element = wrap(scope, handle);
        let key =
            v8::String::new(scope, &index.to_string()).ok_or("native hit array key interrupted")?;
        if result.create_data_property(scope, key.into(), element.into()) != Some(true) {
            return Err("native hit array interrupted".into());
        }
    }
    Ok(result)
}

pub(crate) fn valid_receiver<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Value>,
    kind: u32,
) -> bool {
    let Ok(receiver) = v8::Local::<v8::Object>::try_from(receiver) else {
        return false;
    };
    let _dom = DomScope::for_object(scope, receiver);
    let Some(handle) = handle_of(scope, receiver) else {
        return false;
    };
    with_tree(|tree| match kind {
        1 => tree.node_type_id(handle) == 1,
        2 => {
            tree.node_type_id(handle) == 1
                && tree.namespace_uri(handle) == Some("http://www.w3.org/1999/xhtml")
        }
        3 => {
            tree.node_type_id(handle) == 1
                && tree.namespace_uri(handle) == Some("http://www.w3.org/1999/xhtml")
                && tree.local_name(handle) == Some("input")
        }
        9 => tree.node_type_id(handle) == 9,
        11 => tree.is_shadow_root(handle),
        _ => false,
    })
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_children_preserve_light_order_and_defer_hosts_and_html_slots() {
        let mut tree = Tree::parse("<body><div id=ordinary>before<span>inside</span>after</div><div id=host><b>light</b></div><slot id=slot><i>fallback</i></slot><svg><slot id=svgslot><path></path></slot></svg></body>");
        let ordinary = tree.get_element_by_id("ordinary").unwrap();
        let host = tree.get_element_by_id("host").unwrap();
        let slot = tree.get_element_by_id("slot").unwrap();
        let svg_slot = tree.get_element_by_id("svgslot").unwrap();
        let shadow = tree.attach_shadow(host);
        let shadow_slot = tree.create_element("slot");
        tree.append_child(shadow, shadow_slot);
        for node in [tree.root(), ordinary, svg_slot, shadow] {
            assert_eq!(
                native_composed_children(&tree, node),
                Some(tree.children(node))
            );
        }
        for node in [host, slot, shadow_slot] {
            assert_eq!(native_composed_children(&tree, node), None);
        }
        let text = tree.children(ordinary)[0];
        assert_eq!(tree.node_type_id(text), 3);
        assert_eq!(native_composed_children(&tree, text), Some(Vec::new()));
    }

    #[test]
    fn stylesheet_membership_preserves_original_scope_order_and_refreshes_after_moves() {
        let mut tree = Tree::parse("<head><style id=first></style></head><body><div id=host><style id=light></style></div><style id=last></style></body>");
        let first = tree.get_element_by_id("first").unwrap();
        let light = tree.get_element_by_id("light").unwrap();
        let last = tree.get_element_by_id("last").unwrap();
        let host = tree.get_element_by_id("host").unwrap();
        let shadow = tree.attach_shadow(host);
        let inner = tree.create_element("style");
        tree.append_child(shadow, inner);
        let membership = |tree: &Tree| {
            Composition::scopes(tree)
                .unwrap()
                .scopes
                .into_iter()
                .filter(|(node, _)| matches!(tree.local_name(*node), Some("style" | "link")))
                .collect::<HashSet<_>>()
        };
        let sheets = membership(&tree);
        assert_eq!(
            ordered_stylesheets(&tree, tree.root(), &sheets),
            vec![first, light, last]
        );
        assert_eq!(ordered_stylesheets(&tree, shadow, &sheets), vec![inner]);
        tree.append_child(host, first);
        assert_eq!(
            ordered_stylesheets(&tree, tree.root(), &sheets),
            vec![light, first, last]
        );
        tree.append_child(shadow, light);
        let sheets = membership(&tree);
        assert_eq!(
            ordered_stylesheets(&tree, tree.root(), &sheets),
            vec![first, last]
        );
        assert_eq!(
            ordered_stylesheets(&tree, shadow, &sheets),
            vec![inner, light]
        );
    }
}
