//! Host-only live tree and CSS snapshot access. Included under the binding adapter
//! so native handles and private document ownership never escape to page JS.
use super::*;
use crate::live_layout::{capture_tree, LiveTreeInput};
use crate::live_stylesheets::SheetSnapshot;

pub(crate) struct NativeLayoutInput<'s> {
    pub(crate) owner_context: Option<v8::Local<'s, v8::Context>>,
    pub(crate) tree: LiveTreeInput,
    pub(crate) handle: Handle,
    pub(crate) connected: bool,
    pub(crate) browsing_context: bool,
    pub(crate) sheets: Vec<SheetSnapshot>,
}

pub(crate) fn capture<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<NativeLayoutInput<'s>, String> {
    capture_kind(scope, receiver, 1)
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
    let (tree, connected, root, sheets) = with_tree(|tree| {
        if tree.node_type_id(handle) != kind {
            return Err(String::from("Illegal invocation"));
        }
        let connected = is_connected(tree, handle);
        let input = if connected && browsing_context {
            capture_tree(tree, document_id)?
        } else {
            LiveTreeInput {
                document_id,
                tree_version: tree.version,
                nodes: Vec::new(),
                identities: Vec::new(),
            }
        };
        let mut sheets = Vec::new();
        if connected && browsing_context {
            let mut pending = vec![tree.root()];
            while let Some(next) = pending.pop() {
                let node = turbo_dom_parser::rtdom::NodeRef::new(tree, next);
                let children: Vec<_> = node
                    .children()
                    .into_iter()
                    .map(|child| child.handle())
                    .collect();
                pending.extend(children.into_iter().rev());
                if matches!(node.local_name(), Some("style" | "link")) {
                    sheets.push(node.handle());
                }
            }
        }
        Ok((input, connected, tree.root(), sheets))
    })
    .ok_or("native layout document is unavailable")??;
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
        });
    }
    let document = wrap(scope, root);
    let record = collection_records::get(scope, document)
        .and_then(|record| v8::Local::<v8::Object>::try_from(record).ok())
        .ok_or("native layout document interface is unavailable")?;
    let host = v8::Local::<v8::Object>::try_from(own(scope, record, "cssHost")?)
        .map_err(|_| "native layout CSS host is unavailable")?;
    let mut result = Vec::new();
    for handle in sheets {
        let node = wrap(scope, handle);
        let value = call(scope, host, "sheet", &[node.into()])?;
        if !value.is_null_or_undefined() {
            result.push(export(scope, host, value)?);
        }
    }
    let adopted = call(scope, host, "readAdopted", &[])?;
    let adopted = v8::Local::<v8::Array>::try_from(adopted)
        .map_err(|_| "native CSS adopted snapshot is not an array")?;
    for index in 0..adopted.length() {
        let sheet = adopted
            .get_index(scope, index)
            .ok_or("native CSS adopted read interrupted")?;
        result.push(export(scope, host, sheet)?);
    }
    Ok(NativeLayoutInput {
        owner_context,
        tree,
        handle,
        connected,
        browsing_context,
        sheets: result,
    })
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
    let record = call(scope, host, "exportSheet", &[sheet])?;
    deno_core::serde_v8::from_v8(scope, record)
        .map_err(|error| format!("native CSS snapshot conversion: {error}"))
}

pub(crate) fn wrap_elements<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    ids: &[turbo_html2pdf_core::layout_snapshot::SourceId],
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
        if with_tree(|tree| tree.node_type_id(handle) != 1 || !is_connected(tree, handle))
            .unwrap_or(true)
        {
            return Err("native hit detached or non-element source".into());
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
        9 => tree.node_type_id(handle) == 9,
        _ => false,
    })
    .unwrap_or(false)
}
