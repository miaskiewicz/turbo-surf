//! Host-only transport records. This queue contains no V8 handles and can be
//! drained while the owning isolate is awaiting network IO.
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, Weak,
};
type CancellationOwner = Weak<tokio::sync::watch::Sender<bool>>;
#[derive(Clone, Default)]
pub struct NetworkEventLog {
    pub(crate) rows: Arc<Mutex<VecDeque<serde_json::Value>>>,
    ids: Arc<AtomicU64>,
    revision: Arc<AtomicU64>,
    acknowledged: Arc<AtomicU64>,
    wake_epoch: Arc<AtomicU64>,
    changed: Arc<tokio::sync::Notify>,
    closed: Arc<AtomicBool>,
    worlds: Arc<AtomicU64>,
    host_bodies: Arc<AtomicBool>,
    disposals: Arc<Mutex<Vec<u32>>>,
    lost: Arc<AtomicBool>,
    cancellations: Arc<Mutex<Vec<CancellationOwner>>>,
    activity: crate::network_activity::NetworkActivity,
}
pub(crate) struct NetworkWorldGuard(NetworkEventLog);
impl Drop for NetworkWorldGuard {
    fn drop(&mut self) {
        if self.0.worlds.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.close();
        }
    }
}
impl NetworkEventLog {
    pub(crate) fn register_network_context(&self, context: u32, target: &str) {
        self.activity.register_context(context, target);
    }
    pub(crate) fn begin_context_request(
        &self,
        context: u32,
    ) -> crate::network_activity::RequestActivity {
        self.activity.begin_context(context)
    }
    pub(crate) fn begin_page_request(
        &self,
        target: Option<&str>,
    ) -> crate::network_activity::RequestActivity {
        self.activity.begin(target)
    }
    pub(crate) fn begin_page_navigation(
        &self,
        target: &str,
    ) -> crate::network_activity::RequestActivity {
        self.activity.begin_navigation(target)
    }
    /// Read host-owned target activity without entering or waking a V8 isolate.
    pub fn target_network_state(&self, target: &str) -> serde_json::Value {
        let state = self.activity.snapshot(target);
        serde_json::json!({"pendingRequests":state.requests, "pendingNavigation":state.navigation,
            "revision":state.revision, "error":state.error})
    }

    pub(crate) fn retain_host_bodies(&self) {
        self.host_bodies.store(true, Ordering::Release);
    }
    pub(crate) fn owns_host_bodies(&self) -> bool {
        self.host_bodies.load(Ordering::Acquire)
    }
    /// Publish the final host interception decision through the same response/body
    /// protocol as transport observations. IDs and provenance belong to this hub.
    pub fn publish_intercepted_response(&self, mut header: serde_json::Value, body: &[u8]) {
        if self.is_closed() {
            return;
        }
        let id = self.next_id();
        header["event"] = "response".into();
        header["id"] = id.into();
        header["provenance"] = "intercepted".into();
        header["bodyPending"] = true.into();
        header["bodyIncomplete"] = true.into();
        self.push(header);
        self.push(serde_json::json!({"event":"responsebody","id":id,
            "bodyBytes":body,"body":String::from_utf8_lossy(body),
            "bodyIncomplete":false,"error":null}));
    }
    pub(crate) fn own_world(&self) -> NetworkWorldGuard {
        self.worlds.fetch_add(1, Ordering::Relaxed);
        NetworkWorldGuard(self.clone())
    }
    pub(crate) fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed) + 1
    }
    pub(crate) fn push(&self, row: serde_json::Value) {
        let mut rows = self.rows.lock().unwrap();
        if rows.len() >= 1000 {
            if let Some(previous) = rows.pop_front() {
                if previous["event"] == "console" {
                    self.disposals.lock().unwrap().extend(
                        previous["args"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|arg| {
                                arg["id"].as_u64().and_then(|id| u32::try_from(id).ok())
                            }),
                    );
                } else {
                    self.lost.store(true, Ordering::Release);
                }
            }
        }
        rows.push_back(row);
        self.revision.fetch_add(1, Ordering::Release);
        self.changed.notify_waiters();
    }
    /// Only an isolate owner may release these evicted native argument handles.
    pub fn take_disposals(&self) -> Vec<u32> {
        std::mem::take(&mut *self.disposals.lock().unwrap())
    }
    pub fn discard_handles(&self, ids: Vec<u32>) {
        self.disposals.lock().unwrap().extend(ids);
    }
    pub fn wake(&self) {
        self.wake_epoch.fetch_add(1, Ordering::Release);
        self.changed.notify_waiters();
    }
    pub async fn wait_for_events(&self, observed_wake: u64) -> (u64, bool) {
        let ready = self.changed.notified();
        tokio::pin!(ready);
        ready.as_mut().enable();
        if !self.has_unacknowledged_events()
            && !self.is_closed()
            && self.wake_epoch.load(Ordering::Acquire) == observed_wake
        {
            ready.await;
        }
        (self.wake_epoch.load(Ordering::Acquire), self.is_closed())
    }
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    pub fn has_unacknowledged_events(&self) -> bool {
        self.revision() != self.acknowledged.load(Ordering::Acquire)
    }
    pub fn acknowledge(&self, revision: u64) -> bool {
        if revision > self.revision() {
            return false;
        }
        self.acknowledged.fetch_max(revision, Ordering::Release);
        true
    }
    fn take(&self) -> (u64, Vec<serde_json::Value>) {
        let mut rows = self.rows.lock().unwrap();
        let mut events = Vec::with_capacity(rows.len() + 1);
        if self.lost.swap(false, Ordering::AcqRel) {
            events.push(serde_json::json!({"event":"networkoverflow","message":"Network event queue exceeded capacity"}));
        }
        events.extend(rows.drain(..));
        (self.revision(), events)
    }
    pub fn drain_batch(&self) -> String {
        let (revision, events) = self.take();
        serde_json::json!({"revision":revision.to_string(),"entries":events}).to_string()
    }
    pub fn drain(&self) -> String {
        let (_, events) = self.take();
        serde_json::to_string(&events).unwrap_or_else(|_| "[]".into())
    }
    /// Preserve the snapshot API for callers reading after an engine drain.
    pub(crate) fn drain_snapshot(&self) -> String {
        let (revision, events) = self.take();
        let mut rows: Vec<serde_json::Value> = Vec::new();
        for event in events {
            if event["event"] == "responsebody" {
                if let Some(header) = rows.iter_mut().find(|row| row["id"] == event["id"]) {
                    for name in [
                        "body",
                        "bodyBytes",
                        "bodyIncomplete",
                        "bodyUnavailable",
                        "error",
                    ] {
                        if let Some(value) = event.get(name) {
                            header[name] = value.clone();
                        }
                    }
                    header["bodyPending"] = false.into();
                    continue;
                }
            }
            rows.push(event);
        }
        let value = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into());
        self.acknowledge(revision);
        value
    }
    pub(crate) fn register(&self, cancel: &Arc<tokio::sync::watch::Sender<bool>>) {
        let mut active = self.cancellations.lock().unwrap();
        if self.closed.load(Ordering::Acquire) {
            cancel.send_replace(true);
        } else {
            active.retain(|owner| owner.strong_count() != 0);
            active.push(Arc::downgrade(cancel));
        }
    }
    pub(crate) fn unregister(&self, cancel: &Arc<tokio::sync::watch::Sender<bool>>) {
        let owner = Arc::downgrade(cancel);
        self.cancellations
            .lock()
            .unwrap()
            .retain(|active| !Weak::ptr_eq(active, &owner));
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.wake();
        for cancel in self
            .cancellations
            .lock()
            .unwrap()
            .drain(..)
            .filter_map(|owner| owner.upgrade())
        {
            cancel.send_replace(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn notifications_do_not_lose_events_or_cancellation_before_wait_registration() {
        let log = NetworkEventLog::default();
        log.push(serde_json::json!({"id":1}));
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), log.wait_for_events(0))
                .await
                .unwrap(),
            (0, false)
        );
        log.drain_batch();
        log.acknowledge(log.revision());
        log.wake();
        assert_eq!(log.wait_for_events(0).await, (1, false));
        assert_eq!(log.revision(), 1);
        log.close();
        assert_eq!(log.wait_for_events(1).await, (2, true));
    }
    #[test]
    fn a_replacement_world_keeps_the_queue_open_until_its_last_owner_retires() {
        let log = NetworkEventLog::default();
        let first = log.own_world();
        let replacement = log.own_world();
        drop(first);
        assert!(!log.is_closed());
        drop(replacement);
        assert!(log.is_closed());
    }
    #[test]
    fn cloned_host_queue_drains_once_without_an_isolate() {
        let log = NetworkEventLog::default();
        let reader = log.clone();
        let id = log.next_id();
        std::thread::spawn(move || log.push(serde_json::json!({"id":id})))
            .join()
            .unwrap();
        assert_eq!(reader.drain(), "[{\"id\":1}]");
        assert_eq!(reader.drain(), "[]");
    }
    #[test]
    fn draining_does_not_acknowledge_events_or_erase_later_changes() {
        let log = NetworkEventLog::default();
        log.push(serde_json::json!({"id":1}));
        let batch: serde_json::Value = serde_json::from_str(&log.drain_batch()).unwrap();
        let revision = batch["revision"].as_str().unwrap().parse().unwrap();
        assert!(log.has_unacknowledged_events());
        log.push(serde_json::json!({"id":2}));
        assert!(log.acknowledge(revision));
        assert!(log.has_unacknowledged_events());
        assert!(log.acknowledge(log.revision()));
        assert!(!log.has_unacknowledged_events());
        assert!(!log.acknowledge(log.revision() + 1));
    }
    #[test]
    fn snapshot_coalesces_body_completion_without_changing_admitted_headers() {
        let log = NetworkEventLog::default();
        log.push(serde_json::json!({"event":"response","id":1,"status":200,"bodyPending":true}));
        log.push(serde_json::json!({"event":"responsebody","id":1,"status":0,"error":"broken stream","bodyIncomplete":true}));
        let rows: Vec<serde_json::Value> = serde_json::from_str(&log.drain_snapshot()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["status"], 200);
        assert_eq!(rows[0]["bodyPending"], false);
        assert_eq!(rows[0]["error"], "broken stream");
    }
    #[test]
    fn only_evicted_console_handles_are_queued_for_isolate_disposal() {
        let log = NetworkEventLog::default();
        log.push(serde_json::json!({"event":"console","args":[{"id":42}]}));
        for id in 0..1000 {
            log.push(serde_json::json!({"id":id}));
        }
        assert_eq!(log.take_disposals(), vec![42]);
        assert!(log.take_disposals().is_empty());
        log.push(serde_json::json!({"event":"console","args":[{"id":43}]}));
        log.drain();
        assert!(log.take_disposals().is_empty());
    }
    #[test]
    fn overflowing_host_queue_is_bounded_and_reports_loss() {
        let log = NetworkEventLog::default();
        for id in 0..1001 {
            log.push(serde_json::json!({"id":id}));
        }
        let rows: Vec<serde_json::Value> = serde_json::from_str(&log.drain()).unwrap();
        assert_eq!(rows.len(), 1001);
        assert_eq!(rows[0]["event"], "networkoverflow");
        assert_eq!(rows[1]["id"], 1);
    }
    #[test]
    fn a_host_queue_does_not_keep_a_retired_transport_owner_alive() {
        let log = NetworkEventLog::default();
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let owner = Arc::new(sender);
        log.register(&owner);
        drop(owner);
        assert!(receiver.has_changed().is_err());
    }
    #[test]
    fn close_cancels_a_registered_request_without_entering_the_isolate() {
        let log = NetworkEventLog::default();
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let sender = Arc::new(sender);
        log.register(&sender);
        log.close();
        assert!(*receiver.borrow());
        let (late, late_receiver) = tokio::sync::watch::channel(false);
        let late = Arc::new(late);
        log.register(&late);
        assert!(*late_receiver.borrow());
    }
    #[test]
    fn intercepted_responses_share_native_ids_and_preserve_body_bytes() {
        let log = NetworkEventLog::default();
        let first = log.next_id();
        log.publish_intercepted_response(
            serde_json::json!({"id":first,"event":"console","status":201}),
            &[0, 255],
        );
        let rows: Vec<serde_json::Value> = serde_json::from_str(&log.drain()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["event"], "response");
        assert_eq!(rows[0]["id"], first + 1);
        assert_eq!(rows[0]["provenance"], "intercepted");
        assert_eq!(rows[1]["id"], rows[0]["id"]);
        assert_eq!(rows[1]["bodyBytes"], serde_json::json!([0, 255]));
        log.close();
        log.publish_intercepted_response(serde_json::json!({"status":200}), b"late");
        assert_eq!(log.drain(), "[]");
    }
}
