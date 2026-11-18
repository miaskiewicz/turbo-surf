//! A canceled driver's result belongs to its generation, not a held peer request.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
async fn a_closed_child_waiter_settles_while_an_unrelated_peer_response_stays_held() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    page.eval(
        "globalThis.heldPeer=window.open('', 'held-peer');heldPeer.document.title='held peer';",
    )
    .await
    .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.target != root.to_string() && info.target != child.to_string())
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    assert_eq!(
        page.read_target_engine(peer, "location.assign('/hold');return 'queued'")
            .unwrap(),
        "\"queued\""
    );
    let log = page.network_event_log();
    let mut work = Box::pin(page.poll_owned_work());
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut observed = 0;
        loop {
            let rows: Value = serde_json::from_str(&log.drain()).unwrap();
            log.acknowledge(log.revision());
            if rows
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["event"] == "response" && row["status"] == 200)
            {
                break;
            }
            tokio::select! {
                result = &mut work => panic!("held peer completed before its body: {result:?}"),
                next = log.wait_for_events(observed) => observed = next.0,
            }
        }
    })
    .await
    .expect("actual held peer response headers must arrive");
    // This public turn is interruptible; the actual fetch job remains actor-owned.
    drop(work);
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.as_str())
            .collect::<Vec<_>>(),
        ["/hold"]
    );
    assert!(page.has_pending_auxiliary_work());
    assert_eq!(
        log.target_network_state(&peer.to_string())["pendingNavigation"],
        true
    );
    assert_eq!(page.read_target_engine(child, "Promise.resolve().then(()=>{opener.closeCallbackRan=true;window.close()});return 'armed'").unwrap(), "\"armed\"");
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
        !page
            .auxiliary_event_log()
            .info(&peer.to_string())
            .unwrap()
            .closed
    );
    assert!(
        page.has_pending_auxiliary_work(),
        "the unrelated held request must remain pending"
    );
    assert_eq!(
        log.target_network_state(&peer.to_string())["pendingNavigation"],
        true
    );
    let retained: Value = serde_json::from_str(
        &page
            .read_target_engine(
                root,
                "return [closeCallbackRan,document.title,window.closed]",
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(retained, json!([true, "opener", false]));
    assert_eq!(
        page.read_target_engine(peer, "return document.title")
            .unwrap(),
        "\"held peer\""
    );
    assert!(!server
        .requests()
        .iter()
        .any(|request| request.target == "/must-not-load"));
    page.navigate_target_with_options(
        root,
        &server.url("/healthy-root"),
        "push",
        r#"{"waitUntil":"load","timeoutMs":1000}"#,
    )
    .await
    .unwrap();
    assert_eq!(
        page.read_target_engine(root, "return document.title")
            .unwrap(),
        "\"healthy-root\""
    );
    assert!(page.has_pending_auxiliary_work());
    assert_eq!(
        log.target_network_state(&peer.to_string())["pendingNavigation"],
        true
    );
    assert!(page.close_target(peer).unwrap());
    tokio::time::timeout(Duration::from_secs(5), server.closed.notified())
        .await
        .expect("closing the held peer must close its actual socket");
    fixture::settle(&mut page).await;
    assert!(!page.has_pending_auxiliary_work());
    assert_eq!(
        log.target_network_state(&peer.to_string())["pendingNavigation"],
        false
    );
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.as_str())
            .collect::<Vec<_>>(),
        ["/hold", "/healthy-root"]
    );
    page.close();
}
