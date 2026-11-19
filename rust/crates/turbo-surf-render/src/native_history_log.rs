//! Send-only session-history metadata. Guest state and V8 handles stay in the actor.
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct NativeHistoryLog(Arc<Mutex<BTreeMap<String, Option<Value>>>>);

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
        let mut targets = self
            .0
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

    pub(crate) fn close(&self, target: &str) -> Result<(), String> {
        let mut targets = self
            .0
            .lock()
            .map_err(|_| "Native history metadata lock poisoned")?;
        let target = targets
            .get_mut(target)
            .ok_or("Native target history is not initialized")?;
        *target = None;
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
            let _guard = inner.lock().unwrap();
            panic!("poison fixture");
        })
        .join();
        assert!(log.snapshot("20").unwrap_err().contains("poisoned"));
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
}
