//! Finish only already-admitted root document navigation before collecting it.
use turbo_surf_render::{NetworkEventLog, PageSession};

fn pending(log: &NetworkEventLog, target: &str) -> Result<bool, String> {
    let state = log.target_network_state(target);
    if let Some(error) = state["error"].as_str() {
        return Err(format!("Root navigation accounting unavailable: {error}"));
    }
    state["pendingNavigation"]
        .as_bool()
        .ok_or_else(|| "Root navigation accounting unavailable".to_owned())
}

pub(super) async fn finish(
    page: &mut PageSession,
    deadline: tokio::time::Instant,
) -> Result<(), String> {
    let root = page.auxiliary_root_id()?.to_string();
    let log = page.network_event_log();
    if !pending(&log, &root)? {
        return Ok(());
    }
    tokio::time::timeout_at(deadline, async {
        while pending(&log, &root)? {
            page.poll_root_navigation_work().await?;
            // A scoped turn may complete immediately while work remains admitted.
            tokio::task::yield_now().await;
        }
        Ok(())
    })
    .await
    .map_err(|_| "Root navigation exceeded the render execution deadline".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_uninitialized_network_accounting_is_an_error() {
        let error = pending(&NetworkEventLog::default(), "unknown-root").unwrap_err();
        assert_eq!(error, "Root navigation accounting unavailable: Native Page network ownership is not initialized");
    }
}
