//! Context-owned connectivity; cloned tabs share it, API contexts can fork it.
#[derive(Clone, Debug)]
pub struct NetworkConditions(tokio::sync::watch::Sender<bool>);

impl Default for NetworkConditions {
    fn default() -> Self {
        let (sender, _) = tokio::sync::watch::channel(false);
        Self(sender)
    }
}

impl NetworkConditions {
    pub fn is_offline(&self) -> bool {
        *self.0.borrow()
    }

    /// Return whether an actual transition occurred.
    pub fn set_offline(&self, offline: bool) -> bool {
        self.0.send_if_modified(|current| {
            if *current == offline {
                return false;
            }
            *current = offline;
            true
        })
    }

    pub fn check(&self) -> Result<(), &'static str> {
        if self.is_offline() {
            Err("Network is offline")
        } else {
            Ok(())
        }
    }

    /// Only established WebSocket traffic waits; HTTP never queues for replay.
    pub async fn wait_online(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|offline| !*offline).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn clones_share_transitions_without_losing_reconnect_notifications() {
        let state = NetworkConditions::default();
        let clone = state.clone();
        assert!(state.set_offline(true));
        assert!(!state.set_offline(true));
        assert!(clone.check().is_err());
        let waiting = tokio::spawn(async move {
            clone.wait_online().await;
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        assert!(state.set_offline(false));
        waiting.await.unwrap();
        state.wait_online().await;
        assert!(!state.set_offline(false));
    }
}
