//! Cheap host-owned invalidation for the shim's last serialized live snapshot.
use super::*;
use turbo_dom_browser_env::host_internals::{document_id_of, enter_document_of, with_tree};

#[derive(Default)]
pub(super) struct Revision(pub(super) u64);

pub(super) fn changed(state: &mut OpState) {
    let revision = state.borrow_mut::<Revision>();
    revision.0 = revision.0.wrapping_add(1);
}

#[derive(PartialEq, Eq)]
pub(super) struct Snapshot {
    revision: u64,
    documents: Vec<(u64, u64)>,
}

pub(super) fn snapshot(rt: &mut OwnedRuntime) -> Option<Snapshot> {
    let state = rt.op_state();
    let state = state.borrow();
    let revision = state.borrow::<Revision>().0;
    let frames = state.borrow::<FrameContexts>().contexts.clone();
    drop(state);
    let main = rt.main_context();
    let _activation = rt.activation_key().enter();
    deno_core::scope!(scope, rt);
    let mut contexts = vec![main];
    contexts.extend(frames);
    let mut documents = Vec::with_capacity(contexts.len());
    for context in contexts {
        let context = v8::Local::new(scope, context);
        if context.get_slot::<FrameRetired>().is_some() {
            continue;
        }
        let document = context.get_slot::<ActiveDocument>()?.0.to_local(scope)?;
        let id = document_id_of(scope, document);
        let _document = enter_document_of(scope, document);
        let version = with_tree(|tree| tree.version)?;
        documents.push((id, version));
    }
    Some(Snapshot {
        revision,
        documents,
    })
}

pub(super) fn pending_events(state: &mut OpState) -> bool {
    fetch_streams::refresh_logs(state);
    !state.borrow::<NetworkLog>().0.is_empty()
}

impl PageSession {
    /// Serialize and record exactly the native revisions represented by this snapshot.
    /// Other serialization consumers keep using `serialize` without acknowledging a driver read.
    pub fn serialize_for_driver(&mut self) -> String {
        let html = self.serialize();
        self.driver_snapshot = snapshot(&mut self.rt);
        html
    }

    /// Record the cookie jar actually returned to the driver, including HttpOnly changes.
    pub fn cookies_for_driver(&mut self) -> String {
        let cookies = self.cookies();
        self.driver_cookies = Some(cookies.clone());
        cookies
    }

    fn driver_sync_state(&mut self) -> (bool, bool) {
        let current = snapshot(&mut self.rt);
        let cookies = self.cookies();
        let state = self.rt.op_state();
        let mut state = state.borrow_mut();
        let events = pending_events(&mut state);
        drop(state);
        let messages = has_pending_frame_work(&self.rt);
        let snapshot = current.is_none()
            || current != self.driver_snapshot
            || self.driver_cookies.as_ref() != Some(&cookies)
            || messages;
        (snapshot, events)
    }

    /// Await the actual collector result and inspect native revisions without another
    /// guest evaluation, DOM serialization, event-loop drain or watchdog thread.
    pub async fn eval_expression_tracked(&mut self, expression: &str) -> Result<String, String> {
        let value = self.eval_expression(expression).await?;
        let (snapshot, events) = self.driver_sync_state();
        Ok(
            serde_json::json!({"value": value, "needsSync": snapshot, "events": events})
                .to_string(),
        )
    }
}
