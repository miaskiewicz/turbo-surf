//! Host-owned request lifetimes. Observations remain valid after event queues drain.
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

const REQUEST_LIMIT: usize = 4096;
const CONTEXT_LIMIT: usize = 65536;

#[derive(Clone, Default)]
pub(crate) struct NetworkActivity(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    targets: BTreeMap<String, Target>,
    contexts: BTreeMap<u32, String>,
    unknown: usize,
    unknown_revision: u64,
    active: usize,
    invalid: bool,
}
#[derive(Default)]
struct Target {
    requests: usize,
    navigation: usize,
    revision: u64,
    navigation_revision: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub requests: usize,
    pub navigation: bool,
    pub revision: String,
    pub navigation_revision: String,
    pub error: Option<&'static str>,
}
#[derive(Clone)]
pub(crate) struct RequestActivity(Arc<Request>);
struct Request {
    owner: Option<String>,
    log: NetworkActivity,
    finished: AtomicBool,
    navigation: bool,
    navigation_revision: Option<u64>,
}
impl Drop for Request {
    fn drop(&mut self) {
        self.finish();
    }
}
impl Request {
    fn finish(&self) {
        if !self.finished.swap(true, Ordering::AcqRel) {
            self.log.finish(self.owner.as_deref(), self.navigation);
        }
    }
}
impl RequestActivity {
    pub(crate) fn navigation_revision(&self) -> Option<u64> {
        self.0.navigation_revision
    }
    pub(crate) fn finish(&self) {
        self.0.finish();
    }
}
fn bump(revision: &mut u64) -> bool {
    if let Some(next) = revision.checked_add(1) {
        *revision = next;
        true
    } else {
        false
    }
}
impl NetworkActivity {
    pub(crate) fn register_context(&self, context: u32, target: &str) {
        let mut state = self.0.lock().unwrap();
        if state.contexts.len() >= CONTEXT_LIMIT && !state.contexts.contains_key(&context) {
            state.invalid = true;
            return;
        }
        state.targets.entry(target.to_owned()).or_default();
        state.contexts.insert(context, target.to_owned());
    }
    pub(crate) fn begin_context(&self, context: u32) -> RequestActivity {
        let owner = self.0.lock().unwrap().contexts.get(&context).cloned();
        self.begin(owner.as_deref())
    }
    pub(crate) fn begin(&self, owner: Option<&str>) -> RequestActivity {
        self.start(owner, false)
    }
    pub(crate) fn begin_navigation(&self, owner: &str) -> RequestActivity {
        self.start(Some(owner), true)
    }
    fn start(&self, owner: Option<&str>, navigation: bool) -> RequestActivity {
        let mut state = self.0.lock().unwrap();
        let owner = owner.map(str::to_owned);
        if state.active >= REQUEST_LIMIT {
            state.invalid = true;
        }
        state.active = state.active.saturating_add(1);
        let mut navigation_revision = None;
        if let Some(target) = owner.as_deref() {
            let target = state.targets.entry(target.to_owned()).or_default();
            if navigation {
                target.navigation += 1;
                if bump(&mut target.navigation_revision) {
                    navigation_revision = Some(target.navigation_revision);
                }
            } else {
                target.requests += 1;
            }
            if (navigation && navigation_revision.is_none()) || !bump(&mut target.revision) {
                state.invalid = true;
            }
        } else {
            state.unknown += 1;
            if !bump(&mut state.unknown_revision) {
                state.invalid = true;
            }
        }
        RequestActivity(Arc::new(Request {
            owner,
            log: self.clone(),
            finished: AtomicBool::new(false),
            navigation,
            navigation_revision,
        }))
    }
    fn finish(&self, owner: Option<&str>, navigation: bool) {
        let mut state = self.0.lock().unwrap();
        state.active = state.active.saturating_sub(1);
        if let Some(target) = owner.and_then(|owner| state.targets.get_mut(owner)) {
            if navigation {
                target.navigation = target.navigation.saturating_sub(1);
            } else {
                target.requests = target.requests.saturating_sub(1);
            }
            if !bump(&mut target.revision) {
                state.invalid = true;
            }
        } else {
            state.unknown = state.unknown.saturating_sub(1);
            if !bump(&mut state.unknown_revision) {
                state.invalid = true;
            }
        }
    }
    pub(crate) fn snapshot(&self, owner: &str) -> Snapshot {
        let state = self.0.lock().unwrap();
        let target = state.targets.get(owner);
        let error = if state.invalid {
            Some("Native network activity exceeded its tracking bound")
        } else if state.unknown != 0 {
            Some("Native request has no authoritative Page owner")
        } else if target.is_none() {
            Some("Native Page network ownership is not initialized")
        } else {
            None
        };
        Snapshot {
            navigation_revision: target
                .map_or(0, |target| target.navigation_revision)
                .to_string(),
            requests: target.map_or(0, |target| target.requests),
            navigation: target.is_some_and(|target| target.navigation != 0),
            revision: format!(
                "{}:{}",
                target.map_or(0, |target| target.revision),
                state.unknown_revision
            ),
            error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn log() -> NetworkActivity {
        let log = NetworkActivity::default();
        log.register_context(1, "root");
        log.register_context(2, "child");
        log
    }
    #[test]
    fn navigation_baseline_tracks_only_owned_admissions_not_lifetime_changes() {
        let log = log();
        let initial = log.snapshot("root").navigation_revision;
        let request = log.begin_context(1);
        request.finish();
        assert_eq!(log.snapshot("root").navigation_revision, initial);
        let navigation = log.begin_navigation("root");
        assert_eq!(navigation.navigation_revision(), Some(1));
        assert_eq!(log.snapshot("root").navigation_revision, "1");
        assert_eq!(log.snapshot("child").navigation_revision, "0");
        navigation.finish();
        assert_eq!(log.snapshot("root").navigation_revision, "1");
        let later = log.begin_navigation("root");
        assert_eq!(later.navigation_revision(), Some(2));
    }

    #[test]
    fn navigation_baseline_overflow_is_explicit_and_never_reuses_a_token() {
        let log = log();
        log.0
            .lock()
            .unwrap()
            .targets
            .get_mut("root")
            .unwrap()
            .navigation_revision = u64::MAX;
        let navigation = log.begin_navigation("root");
        assert_eq!(navigation.navigation_revision(), None);
        assert!(log.snapshot("root").error.is_some());
    }

    #[test]
    fn requests_are_busy_before_headers_until_true_completion() {
        let log = log();
        let initial = log.snapshot("root");
        let request = log.begin_context(1);
        assert_eq!(log.snapshot("root").requests, 1);
        assert_ne!(log.snapshot("root").revision, initial.revision);
        request.finish();
        assert_eq!(log.snapshot("root").requests, 0);
        let finished = log.snapshot("root").revision;
        request.finish();
        drop(request);
        assert_eq!(log.snapshot("root").revision, finished);
    }
    #[test]
    fn sibling_work_does_not_reset_a_pages_quiet_interval() {
        let log = log();
        let root = log.snapshot("root");
        let child = log.begin_context(2);
        let _navigation = log.begin_navigation("child");
        assert_eq!(log.snapshot("root"), root);
        drop(child);
        assert_eq!(log.snapshot("root"), root);
    }
    #[test]
    fn guard_drop_and_navigation_retirement_release_pending_work() {
        let log = log();
        let request = log.begin_context(1);
        let copy = request.clone();
        drop(request);
        assert_eq!(log.snapshot("root").requests, 1);
        drop(copy);
        assert_eq!(log.snapshot("root").requests, 0);
        let first = log.begin_navigation("root");
        let second = log.begin_navigation("root");
        assert!(log.snapshot("root").navigation);
        drop(first);
        assert!(log.snapshot("root").navigation);
        drop(second);
        assert!(!log.snapshot("root").navigation);
    }
    #[test]
    fn missing_or_unknown_provenance_never_reports_idle() {
        let log = log();
        assert!(log.snapshot("other").error.is_some());
        let request = log.begin_context(3);
        assert!(log.snapshot("root").error.is_some());
        drop(request);
        assert!(log.snapshot("root").error.is_none());
    }
    #[test]
    fn tracking_capacity_fails_closed_instead_of_silently_dropping_requests() {
        let log = log();
        let requests = (0..=REQUEST_LIMIT)
            .map(|_| log.begin_context(1))
            .collect::<Vec<_>>();
        assert!(log.snapshot("root").error.is_some());
        drop(requests);
        assert!(log.snapshot("root").error.is_some());
    }
}
