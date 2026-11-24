//! Send-only session-history metadata. Guest state and V8 handles stay in the actor.
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

#[derive(Clone, Default)]
pub struct NativeHistoryLog(Arc<HistorySnapshots>);

#[derive(Default)]
struct HistorySnapshots {
    targets: Mutex<BTreeMap<String, Option<Value>>>,
    changed: Condvar,
}

struct PublicationWake<'a>(&'a Condvar);

impl Drop for PublicationWake<'_> {
    fn drop(&mut self) {
        self.0.notify_all();
    }
}

impl NativeHistoryLog {
    pub(crate) fn update(&self, target: &str, snapshot: Value) -> Result<(), String> {
        if target.is_empty() || !snapshot.is_object() {
            return Err("Native history metadata identity is unavailable".into());
        }
        self.publish_with(target, || Ok(snapshot))
    }

    /// The closure performs only a prepared native commit and metadata construction.
    /// Clone adapters, guest code and fallible preparation must finish before this lock.
    pub(crate) fn publish_with(
        &self,
        target: &str,
        update: impl FnOnce() -> Result<Value, String>,
    ) -> Result<(), String> {
        if target.is_empty() {
            return Err("Native history metadata identity is unavailable".into());
        }
        // Notify after the lock drops even if a native publication unwinds.
        let _wake = PublicationWake(&self.0.changed);
        let mut targets = self
            .0
            .targets
            .lock()
            .map_err(|_| "Native history metadata lock poisoned")?;
        if targets.get(target).is_some_and(Option::is_none) {
            return Err("Native history target is closed".into());
        }
        let snapshot = update()?;
        targets.insert(target.into(), Some(snapshot));
        Ok(())
    }

    pub fn snapshot(&self, target: &str) -> Result<Value, String> {
        let targets = self
            .0
            .targets
            .lock()
            .map_err(|_| "Native history metadata lock poisoned")?;
        let snapshot = targets
            .get(target)
            .ok_or("Native target history is not initialized")?
            .as_ref()
            .ok_or("Native history target is closed")?;
        if snapshot.get("pendingTransaction").is_some() {
            return Err("Native history mutation is awaiting private acknowledgement".into());
        }
        Ok(snapshot.clone())
    }

    /// Wait on publication, never behind an actor job that may need the host's IO.
    /// Only the actor's private ACK or retirement can make a pending read ready.
    pub fn snapshot_acknowledged(
        &self,
        target: &str,
        deadline: Option<Instant>,
    ) -> Result<Value, String> {
        self.snapshot_acknowledged_with(target, deadline, || {})
    }

    fn snapshot_acknowledged_with(
        &self,
        target: &str,
        deadline: Option<Instant>,
        mut pending: impl FnMut(),
    ) -> Result<Value, String> {
        let mut targets = self
            .0
            .targets
            .lock()
            .map_err(|_| "Native history metadata lock poisoned")?;
        loop {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err("Native history acknowledgement wait timed out".into());
            }
            let snapshot = targets
                .get(target)
                .ok_or("Native target history is not initialized")?
                .as_ref()
                .ok_or("Native history target is closed")?;
            if snapshot.get("pendingTransaction").is_none() {
                return Ok(snapshot.clone());
            }
            pending();
            targets = if let Some(deadline) = deadline {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or("Native history acknowledgement wait timed out")?;
                self.0
                    .changed
                    .wait_timeout(targets, remaining)
                    .map_err(|_| "Native history metadata lock poisoned")?
                    .0
            } else {
                self.0
                    .changed
                    .wait(targets)
                    .map_err(|_| "Native history metadata lock poisoned")?
            };
        }
    }

    pub(crate) fn close(&self, target: &str) -> Result<(), String> {
        let mut targets = self
            .0
            .targets
            .lock()
            .map_err(|_| "Native history metadata lock poisoned")?;
        let target = targets
            .get_mut(target)
            .ok_or("Native target history is not initialized")?;
        *target = None;
        self.0.changed.notify_all();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn aliases_share_only_owned_metadata_and_closed_rows_cannot_reopen() {
        let log = NativeHistoryLog::default();
        let clone = log.clone();
        assert!(clone.snapshot("20").is_err());
        log.update("20", json!({"revision":"1","entries":[{"url":"/one"}]}))
            .unwrap();
        let mut snapshot = clone.snapshot("20").unwrap();
        snapshot["revision"] = "tampered".into();
        assert_eq!(log.snapshot("20").unwrap()["revision"], "1");
        assert!(log.snapshot("21").is_err());
        clone.close("20").unwrap();
        assert!(log.snapshot("20").is_err());
        assert!(log.update("20", json!({"revision":"2"})).is_err());
    }
    #[test]
    fn failed_prepared_commit_preserves_the_last_published_snapshot() {
        let log = NativeHistoryLog::default();
        log.update("20", json!({"revision":"1"})).unwrap();
        assert!(log
            .publish_with("20", || Err("stale native revision".into()))
            .is_err());
        assert_eq!(log.snapshot("20").unwrap()["revision"], "1");
        log.close("20").unwrap();
        let called = std::cell::Cell::new(false);
        assert!(log
            .publish_with("20", || {
                called.set(true);
                Ok(json!({}))
            })
            .is_err());
        assert!(!called.get());
    }

    #[test]
    fn host_reads_reject_unacknowledged_native_metadata_until_private_commit() {
        let log = NativeHistoryLog::default();
        log.update("20", json!({"revision":"1"})).unwrap();
        log.update(
            "20",
            json!({"revision":"2","pendingTransaction":"native-token"}),
        )
        .unwrap();
        assert!(log.snapshot("20").unwrap_err().contains("acknowledgement"));
        log.update("20", json!({"revision":"2"})).unwrap();
        assert_eq!(log.snapshot("20").unwrap()["revision"], "2");
    }

    #[test]
    fn poisoned_metadata_is_an_error() {
        let log = NativeHistoryLog::default();
        let inner = log.0.clone();
        let _ = std::thread::spawn(move || {
            let _guard = inner.targets.lock().unwrap();
            panic!("poison fixture");
        })
        .join();
        assert!(log.snapshot("20").unwrap_err().contains("poisoned"));
        assert!(log
            .snapshot_acknowledged("20", None)
            .unwrap_err()
            .contains("poisoned"));
        assert!(log.update("20", json!({})).is_err());
        assert!(log.close("20").is_err());
        let called = std::cell::Cell::new(false);
        assert!(log
            .publish_with("20", || {
                called.set(true);
                Ok(json!({}))
            })
            .is_err());
        assert!(!called.get());
    }

    fn pending_read(
        log: &NativeHistoryLog,
        target: &str,
    ) -> (
        std::thread::JoinHandle<Result<Value, String>>,
        std::sync::mpsc::Receiver<()>,
    ) {
        let (sent, received) = std::sync::mpsc::channel();
        let reader = log.clone();
        let target = target.to_string();
        let task = std::thread::spawn(move || {
            reader.snapshot_acknowledged_with(
                &target,
                Some(Instant::now() + std::time::Duration::from_secs(2)),
                || {
                    sent.send(()).unwrap();
                },
            )
        });
        (task, received)
    }

    #[test]
    fn acknowledged_read_waits_for_exact_target_publication_not_another_target() {
        let log = NativeHistoryLog::default();
        log.update("20", json!({"revision":"2","pendingTransaction":"ticket"}))
            .unwrap();
        let (task, waiting) = pending_read(&log, "20");
        waiting
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        log.update("21", json!({"revision":"other"})).unwrap();
        assert!(log.snapshot("20").unwrap_err().contains("acknowledgement"));
        log.update("20", json!({"revision":"2","entries":[{"url":"/actual"}]}))
            .unwrap();
        let result = task.join().unwrap().unwrap();
        assert_eq!(result["revision"], "2");
        assert_eq!(result["entries"][0]["url"], "/actual");
        assert!(result.get("pendingTransaction").is_none());
    }

    #[test]
    fn acknowledged_read_wakes_on_retirement_and_keeps_closed_error() {
        let log = NativeHistoryLog::default();
        log.update("20", json!({"revision":"2","pendingTransaction":"ticket"}))
            .unwrap();
        let (task, waiting) = pending_read(&log, "20");
        waiting
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        log.close("20").unwrap();
        assert!(task.join().unwrap().unwrap_err().contains("closed"));
    }

    #[test]
    fn acknowledged_read_timeout_never_acknowledges_or_replays_mutation() {
        let log = NativeHistoryLog::default();
        log.update("20", json!({"revision":"2","pendingTransaction":"ticket"}))
            .unwrap();
        assert!(log
            .snapshot_acknowledged(
                "20",
                Some(Instant::now() + std::time::Duration::from_millis(1))
            )
            .unwrap_err()
            .contains("timed out"));
        assert!(log.snapshot("20").unwrap_err().contains("acknowledgement"));
        log.update("20", json!({"revision":"2"})).unwrap();
        assert_eq!(
            log.snapshot_acknowledged("20", None).unwrap()["revision"],
            "2"
        );
        assert!(log
            .snapshot_acknowledged("missing", None)
            .unwrap_err()
            .contains("not initialized"));
    }
    #[test]
    fn pending_read_wakes_with_poison_error_if_publication_unwinds() {
        let log = NativeHistoryLog::default();
        log.update("20", json!({"revision":"2","pendingTransaction":"ticket"}))
            .unwrap();
        let (task, waiting) = pending_read(&log, "20");
        waiting
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        let writer = log.clone();
        assert!(std::thread::spawn(move || {
            let _ = writer.publish_with("20", || panic!("native publication fixture"));
        })
        .join()
        .is_err());
        assert!(task.join().unwrap().unwrap_err().contains("poisoned"));
    }
}
