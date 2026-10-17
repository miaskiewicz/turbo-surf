//! Label associations use the actual light-tree root, never guest selectors.
use super::*;
fn rows(root: Handle) -> Vec<Handle> {
    with_tree(|tree| {
        let mut pending = vec![root];
        let mut result = Vec::new();
        while let Some(node) = pending.pop() {
            result.push(node);
            pending.extend(tree.children(node).into_iter().rev());
        }
        result
    })
    .unwrap_or_default()
}
fn root(mut node: Handle) -> Handle {
    with_tree(|tree| {
        while let Some(parent) = tree.parent(node) {
            node = parent;
        }
        node
    })
    .unwrap_or(node)
}
pub(super) fn hidden(node: Handle) -> bool {
    with_tree(|tree| {
        tree.namespace_id(node) == 0
            && tree.local_name(node) == Some("input")
            && tree
                .get_attribute_ns_borrowed(node, None, "type")
                .is_some_and(|value| value.eq_ignore_ascii_case("hidden"))
    })
    .unwrap_or(false)
}
fn labelable(scope: &mut v8::PinScope, node: Handle) -> bool {
    let kind = with_tree(|tree| {
        if tree.namespace_id(node) != 0 {
            return None;
        }
        let name = tree.local_name(node)?;
        if name == "input"
            && tree
                .get_attribute_ns_borrowed(node, None, "type")
                .is_some_and(|value| value.eq_ignore_ascii_case("hidden"))
        {
            return None;
        }
        Some((
            matches!(
                name,
                "input" | "button" | "textarea" | "select" | "output" | "meter" | "progress"
            ),
            name.contains('-'),
        ))
    })
    .flatten();
    match kind {
        Some((true, _)) => true,
        Some((false, true)) => {
            let object = DOM
                .with(|dom| dom.borrow().as_ref()?.cache.get(&node).cloned())
                .and_then(|object| object.to_local(scope));
            let Some(object) = object else {
                return false;
            };
            let name = v8::String::new(scope, "turbo.forms.associated-record").unwrap();
            let key = v8::Private::for_api(scope, Some(name));
            object
                .get_private(scope, key)
                .is_some_and(|value| !value.is_undefined())
        }
        _ => false,
    }
}
pub(super) fn control(scope: &mut v8::PinScope, label: Handle) -> Option<Handle> {
    if !with_tree(|tree| tree.namespace_id(label) == 0 && tree.local_name(label) == Some("label"))
        .unwrap_or(false)
    {
        return None;
    }
    if let Some(id) = with_tree(|tree| tree.get_attribute_ns_utf16(label, None, "for")).flatten() {
        if id.is_empty() {
            return None;
        }
        let target = rows(root(label)).into_iter().find(|node| {
            with_tree(|tree| {
                tree.get_attribute_ns_utf16(*node, None, "id").as_deref() == Some(id.as_slice())
            })
            .unwrap_or(false)
        })?;
        return labelable(scope, target).then_some(target);
    }
    rows(label)
        .into_iter()
        .skip(1)
        .find(|node| labelable(scope, *node))
}
pub(super) fn labels(scope: &mut v8::PinScope, host: Handle) -> Vec<Handle> {
    if !labelable(scope, host) {
        return Vec::new();
    }
    rows(root(host))
        .into_iter()
        .skip(1)
        .filter(|label| control(scope, *label) == Some(host))
        .collect()
}
