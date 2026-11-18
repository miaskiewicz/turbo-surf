//! Actual child closure cancels its admitted driver navigation before wire work.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};

#[tokio::test(flavor = "current_thread")]
async fn a_queued_child_close_settles_its_navigation_waiter_without_contacting_http() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    // fixture::child creates a genuine script-opened Window, eligible for close().
    let child = fixture::child(&mut page).await;
    let armed = page
        .read_target_engine(
            child,
            "globalThis.closeRan=false;Promise.resolve().then(()=>{closeRan=true;opener.closeCallbackRan=true;window.close()});return closeRan",
        )
        .unwrap();
    assert_eq!(
        armed, "false",
        "the synchronous host read must not checkpoint"
    );
    assert!(
        !page
            .auxiliary_event_log()
            .info(&child.to_string())
            .unwrap()
            .closed
    );
    assert!(server.requests().is_empty());

    let error = page
        .navigate_target_with_options(
            child,
            &server.url("/must-not-load"),
            "push",
            r#"{"waitUntil":"load","timeoutMs":1000}"#,
        )
        .await
        .unwrap_err();
    assert_eq!(error, "Navigation target closed or superseded");
    assert!(
        page.auxiliary_event_log()
            .info(&child.to_string())
            .unwrap()
            .closed
    );
    assert!(
        server.requests().is_empty(),
        "canceled lazy fetch reached HTTP"
    );
    let root_state: Value = serde_json::from_str(
        &page
            .read_target_engine(
                root,
                "return [closeCallbackRan,document.title,window.closed]",
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(root_state, json!([true, "opener", false]));
    assert!(!page.has_pending_auxiliary_work());

    let next = fixture::child(&mut page).await;
    assert_ne!(next, child);
    page.navigate_target_with_options(
        next,
        &server.url("/accepted-after-close"),
        "push",
        r#"{"waitUntil":"load","timeoutMs":1000}"#,
    )
    .await
    .unwrap();
    assert_eq!(
        page.read_target_engine(next, "return document.title")
            .unwrap(),
        "\"accepted-after-close\""
    );
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.as_str())
            .collect::<Vec<_>>(),
        ["/accepted-after-close"]
    );
    assert_eq!(
        page.read_target_engine(root, "return document.title")
            .unwrap(),
        "\"opener\""
    );
    page.close();
}
