//! The exact authored fixture/probe used by a genuine Chrome control.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::{
    BrowserStorage, DocumentResponseMetadata, PageSession, PageSessionOptions, WindowTimerMode,
};

#[tokio::test(flavor = "current_thread")]
async fn timer_task_semantics_match_the_source_pinned_chrome_control() {
    let html = include_str!("../../../../harness/detection/window-timer-semantics.html");
    let probe = include_str!("../../../../harness/detection/window-timer-semantics-probe.js");
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/window-timer-semantics-chrome155.json"
    ))
    .unwrap();
    for (name, bytes) in [
        ("window-timer-semantics.html", html.as_bytes()),
        ("window-timer-semantics-probe.js", probe.as_bytes()),
    ] {
        assert_eq!(
            control["sourceHashes"][name],
            format!("{:x}", Sha256::digest(bytes)),
            "{name}"
        );
    }
    let mut page = PageSession::open_with_document_response_and_options(
        html,
        "https://timer.test/",
        "",
        "",
        10000,
        "",
        ClientHints::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let encoded: String =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let actual: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(actual["semantics"], control["result"]["semantics"]);
    assert_eq!(actual["protocol"], control["result"]["protocol"]);
    // Each implementation records its actual scheduling latency. Exact elapsed
    // milliseconds are not a correctness oracle or permission to renew a deadline.
    for name in [
        "direct",
        "promise",
        "queueMicrotask",
        "crossRealm",
        "crossRealmPromise",
        "crossRealmQueueMicrotask",
        "interval",
    ] {
        let rows = actual["timing"][name].as_array().unwrap();
        assert_eq!(rows.len(), 12, "{name}");
        assert!(rows
            .iter()
            .all(|value| value.as_f64().is_some_and(|ms| ms.is_finite() && ms >= 0.0)));
    }
}
