//! Native allocation limits for guest-created windows, independent of the JS heap.
use super::*;

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AuxiliaryResourcePolicy {
    pub max_pages: usize,
    pub max_history_entries: usize,
    pub max_creations: usize,
    pub max_pending_navigations: usize,
    pub max_event_rows: usize,
    pub max_navigation_url_bytes: usize,
}

impl Default for AuxiliaryResourcePolicy {
    fn default() -> Self {
        Self {
            max_pages: 32,
            max_history_entries: 256,
            max_creations: 128,
            max_pending_navigations: 64,
            max_event_rows: 1024,
            max_navigation_url_bytes: 16 * 1024,
        }
    }
}

pub(super) struct Reservation(Rc<RefCell<OpState>>, bool);
impl Drop for Reservation {
    fn drop(&mut self) {
        if self.1 {
            let mut state = self.0.borrow_mut();
            let pages = state.borrow_mut::<auxiliary_pages::Pages>();
            pages.reserved = pages.reserved.saturating_sub(1);
        }
    }
}

pub(super) fn reserve(
    scope: &mut v8::PinScope,
    request: &turbo_dom_browser_env::host_internals::OpenRequest,
) -> Option<Reservation> {
    let state = JsRuntime::op_state_from(scope);
    {
        let mut state = state.borrow_mut();
        let pages = state.borrow_mut::<auxiliary_pages::Pages>();
        let policy = pages.policy;
        if !pages.enabled
            || pages.events.is_closed()
            || pages.events.overflowed()
            || policy.max_event_rows == 0
            || pages.events.queued_len().saturating_add(2) > policy.max_event_rows
        {
            return None;
        }
        if request.navigate
            && (request.url.len() > policy.max_navigation_url_bytes
                || pages.pending_count() >= policy.max_pending_navigations)
        {
            return None;
        }
        if request.new_context {
            if pages.auxiliary_count() + pages.reserved >= policy.max_pages
                || pages.created >= policy.max_creations
            {
                return None;
            }
            pages.created += 1;
            pages.reserved += 1;
        }
    }
    Some(Reservation(state, request.new_context))
}
