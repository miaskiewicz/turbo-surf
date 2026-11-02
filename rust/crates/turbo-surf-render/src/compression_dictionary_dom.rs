//! Private composed resource enumeration over the public native Tree surface,
//! reached through the binding's host-only `host_internals` (never page JS).
use turbo_dom_browser_env::host_internals::{enter_document_of, handle_of, with_tree, wrap};

pub(crate) fn links<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<v8::Object>,
) -> Result<Vec<v8::Local<'s, v8::Object>>, &'static str> {
    let _owner = enter_document_of(scope, document);
    let root = handle_of(scope, document).ok_or("Dictionary document unavailable")?;
    // Discovery runs for every document before its resources load, so it has no
    // bound that could fail it (turbo-surf#75): the walk is linear in the tree, like
    // the parse that built it, and every link settles like Chrome's. Fetches stay
    // bounded by op_dictionary_prepare's distinct-target budget and request sharing.
    let handles = with_tree(|tree| {
        if !matches!(tree.node_type_id(root), 1 | 9 | 11) {
            return Vec::new();
        }
        let mut pending = vec![root];
        let mut found = Vec::new();
        while let Some(node) = pending.pop() {
            if tree.namespace_uri(node) == Some("http://www.w3.org/1999/xhtml")
                && tree.local_name(node) == Some("link")
                && tree.get_attribute(node, "rel").is_some_and(|rel| {
                    rel.split([' ', '\t', '\n', '\r', '\u{000c}'])
                        .any(|token| token.eq_ignore_ascii_case("compression-dictionary"))
                })
            {
                found.push(node);
            }
            if let Some(shadow) = tree.shadow_root(node) {
                pending.push(shadow);
            }
            pending.extend(tree.children(node).into_iter().rev());
        }
        found
    })
    .ok_or("Dictionary native document unavailable")?;
    Ok(handles
        .into_iter()
        .map(|handle| wrap(scope, handle))
        .collect())
}
