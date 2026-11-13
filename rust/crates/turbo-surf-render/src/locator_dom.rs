//! A cheap native revision for the trusted locator index, including shadow mutations.
use deno_core::{op2, v8};
use deno_error::JsErrorBox;
use turbo_dom_browser_env::host_internals::{enter_document_of, with_tree};

#[op2]
#[string]
pub(crate) fn op_locator_revision(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
) -> Result<String, JsErrorBox> {
    let _document = enter_document_of(scope, document);
    with_tree(|tree| tree.version.to_string()).ok_or_else(unavailable)
}

#[op2(fast)]
pub(crate) fn op_locator_has_shadow(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
) -> Result<bool, JsErrorBox> {
    let _document = enter_document_of(scope, document);
    with_tree(|tree| tree.has_shadow_roots()).ok_or_else(unavailable)
}

fn unavailable() -> JsErrorBox {
    JsErrorBox::generic("Native locator document is unavailable")
}
