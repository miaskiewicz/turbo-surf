//! History identities are bound to the actual Document, independently of DOM-id sentinels.
use super::*;

#[derive(Default)]
struct NextDocument(u64);
struct DocumentIdentity {
    document: v8::Weak<v8::Object>,
    id: u64,
}

pub(in crate::runtime) fn identity<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    document: v8::Local<'s, v8::Object>,
) -> Result<u64, JsErrorBox> {
    let active = context
        .get_slot::<ActiveDocument>()
        .and_then(|active| active.0.to_local(scope));
    if active != Some(document)
        || document.get_creation_context(scope) != Some(context)
        || context.get_slot::<FrameRetired>().is_some()
    {
        return Err(failure(
            "Native history physical document ownership mismatch",
        ));
    }
    if let Some(identity) = context.get_slot::<DocumentIdentity>() {
        if identity.document.to_local(scope) == Some(document) {
            return Ok(identity.id);
        }
    }
    let id = {
        let shared = JsRuntime::op_state_from(scope);
        let mut state = shared.borrow_mut();
        if !state.has::<NextDocument>() {
            state.put(NextDocument::default());
        }
        let next = state.borrow_mut::<NextDocument>();
        let id = next
            .0
            .checked_add(1)
            .ok_or_else(|| failure("Native history physical document identity exhausted"))?;
        next.0 = id;
        id
    };
    let document = v8::Weak::new(scope, document);
    context.set_slot(Rc::new(DocumentIdentity { document, id }));
    Ok(id)
}
