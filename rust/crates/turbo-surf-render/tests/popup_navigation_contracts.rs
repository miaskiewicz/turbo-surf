#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::{AuxiliaryBootstrap, AuxiliaryPageEvent};

#[tokio::test(flavor = "current_thread")]
async fn a_child_navigation_sends_actual_binary_bytes_and_correlates_the_response() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let target = fixture::child(&mut page).await;
    page.auxiliary_event_log().take();
    let options = json!({"method":"post","body":"not the binary body","bodyBytes":[0,255,10],"contentType":"application/octet-stream","headers":{"x-owned-contract":"yes"}}).to_string();
    let receipt: Value = serde_json::from_str(
        &page
            .navigate_target_with_options(target, &server.url("/post"), "replace", &options)
            .await
            .unwrap(),
    )
    .unwrap();
    fixture::settle(&mut page).await;
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].target, "/post");
    assert_eq!(
        requests[0].headers["content-type"],
        "application/octet-stream"
    );
    assert_eq!(requests[0].headers["x-owned-contract"], "yes");
    assert_eq!(requests[0].body, [0, 255, 10]);
    assert_eq!(receipt["status"], 200);
    assert_eq!(receipt["finalUrl"], server.url("/post"));
    assert_eq!(receipt["attachment"], false);
    let records: Value = serde_json::from_str(&page.drain_network_log()).unwrap();
    let response = records
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == receipt["requestId"] && record["event"] == "response")
        .unwrap();
    assert_eq!(response["status"], 200);
    assert_eq!(response["context"]["navigation"], true);
    assert_eq!(response["context"]["frame"]["pageId"], target.to_string());
    assert_eq!(
        page.eval_expression("document.title").await.unwrap(),
        "opener"
    );
    let events = page.auxiliary_event_log().take();
    assert!(events.iter().any(|event| matches!(event, AuxiliaryPageEvent::DomContentLoaded { target: id } if id == &target.to_string())));
    assert!(events.iter().any(|event| matches!(event, AuxiliaryPageEvent::Load { target: id } if id == &target.to_string())));
}

#[tokio::test(flavor = "current_thread")]
async fn history_push_replace_back_and_forward_follow_the_addressed_page_only() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let target = fixture::child(&mut page).await;
    for (path, mode) in [
        ("/one", "push"),
        ("/two", "push"),
        ("/three", "replace"),
        ("/one", "back"),
        ("/three", "forward"),
    ] {
        let receipt: Value = serde_json::from_str(
            &page
                .navigate_target(target, &server.url(path), mode)
                .await
                .unwrap(),
        )
        .unwrap();
        fixture::settle(&mut page).await;
        assert_eq!(receipt["finalUrl"], server.url(path));
        assert_eq!(
            page.eval_target_expression(target, "document.title", false, false)
                .await
                .unwrap(),
            path.trim_start_matches('/')
        );
    }
    assert!(page
        .navigate_target(target, "", "forward")
        .await
        .unwrap_err()
        .contains("out of range"));
    assert_eq!(page.eval_expression("child.closed").await.unwrap(), "false");
    assert_eq!(
        page.eval_expression("child === window.open('', 'contract-child')")
            .await
            .unwrap(),
        "true"
    );
    assert!(page.close_target(root).unwrap());
    page.navigate_target(target, &server.url("/surviving"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        page.eval_target_expression(target, "document.title", false, false)
            .await
            .unwrap(),
        "surviving"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn context_and_page_bootstrap_sources_remain_distinct_across_navigation() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    page.configure_target_bootstrap(
        root,
        AuxiliaryBootstrap {
            init_script: "globalThis.pageOnly='opener-only'".into(),
            ..Default::default()
        },
    )
    .unwrap();
    page.configure_auxiliary_bootstrap(AuxiliaryBootstrap {
        init_script: "globalThis.contextValue='context'".into(),
        ..Default::default()
    })
    .unwrap();
    let target = fixture::child(&mut page).await;
    let initial = page
        .eval_target_expression(
            target,
            "[contextValue,globalThis.pageOnly??null]",
            false,
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&initial).unwrap(),
        json!(["context", null])
    );
    page.configure_target_bootstrap(
        target,
        AuxiliaryBootstrap {
            init_script: "globalThis.pageOnly='child-only'".into(),
            ..Default::default()
        },
    )
    .unwrap();
    page.navigate_target(target, &server.url("/bootstrap"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    let observation = page
        .eval_target_expression(target, "bootObservation", false, true)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&observation).unwrap(),
        json!(["context", "child-only"])
    );
    assert_eq!(
        page.eval_expression("typeof pageOnly").await.unwrap(),
        "undefined"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_attachment_keeps_original_document_and_returns_its_actual_bytes() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let target = fixture::child(&mut page).await;
    page.auxiliary_event_log().take();
    let receipt: Value = serde_json::from_str(
        &page
            .navigate_target(target, &server.url("/attachment"), "replace")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["attachment"], true);
    assert_eq!(receipt["bodyBytes"], json!([0, 255, 65, 10]));
    assert_eq!(receipt["status"], 200);
    assert_eq!(
        page.eval_target_expression(target, "[document.title,location.href]", false, true)
            .await
            .unwrap(),
        "[\"original\",\"about:blank\"]"
    );
    assert!(!page
        .auxiliary_event_log()
        .take()
        .iter()
        .any(|event| matches!(event, AuxiliaryPageEvent::Navigated { .. })));
}

#[tokio::test(flavor = "current_thread")]
async fn coop_detaches_the_old_proxy_without_closing_the_real_driver_page() {
    let source = fixture::Server::start().await;
    let destination = fixture::Server::start().await;
    let mut page = fixture::open(&source.origin, Default::default()).await;
    let target = fixture::child(&mut page).await;
    page.navigate_target(target, &source.url("/one"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(page.eval_expression("child.closed").await.unwrap(), "false");
    page.navigate_target(target, &destination.url("/coop"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(page.eval_expression("child.closed").await.unwrap(), "true");
    assert!(
        !page
            .auxiliary_event_log()
            .info(&target.to_string())
            .unwrap()
            .closed
    );
    let result = page
        .eval_target_expression(target, "[document.title,window.opener===null]", false, true)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!(["isolated", true])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_pending_navigation_closes_http_without_replacing_its_document() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let target = fixture::child(&mut page).await;
    let log = page.network_event_log();
    log.drain();
    log.acknowledge(log.revision());
    let url = server.url("/hold");
    let mut navigation = Box::pin(page.navigate_target(target, &url, "replace"));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::select! {
            _=log.wait_for_events(0)=>{},
            result=&mut navigation=>panic!("held response completed before cancellation: {result:?}"),
        }
    }).await.expect("real response headers must arrive before cancellation");
    let headers: Value = serde_json::from_str(&log.drain()).unwrap();
    assert!(headers
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["event"] == "response" && record["status"] == 200));
    drop(navigation);
    tokio::time::timeout(std::time::Duration::from_secs(5), server.closed.notified())
        .await
        .expect("native navigation cancellation must close its actual socket");
    assert!(!page.has_pending_auxiliary_work());
    assert_eq!(
        page.eval_target_expression(target, "document.title", false, false)
            .await
            .unwrap(),
        "original"
    );
    let completed: Value = serde_json::from_str(&log.drain()).unwrap();
    assert!(completed
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["event"] == "responsebody" && record["bodyUnavailable"] == true));
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_navigation_inputs_fail_without_contacting_the_fixture() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let target = fixture::child(&mut page).await;
    assert!(page
        .navigate_target_with_options(target, &server.url("/one"), "replace", "{")
        .await
        .unwrap_err()
        .contains("options"));
    assert!(page
        .navigate_target_with_options(
            target,
            &server.url("/one"),
            "replace",
            "{\"waitUntil\":\"never\"}"
        )
        .await
        .unwrap_err()
        .contains("waitUntil"));
    assert!(page
        .navigate_target(target, &server.url("/one"), "teleport")
        .await
        .unwrap_err()
        .contains("history"));
    assert!(page
        .navigate_target(target, "http://[", "replace")
        .await
        .unwrap_err()
        .contains("URL"));
    assert!(page
        .set_target_content(target, "<p>no</p>", "commit", None)
        .await
        .unwrap_err()
        .contains("waitUntil"));
    assert!(server.requests().is_empty());
    assert_eq!(
        page.eval_target_expression(target, "document.title", false, false)
            .await
            .unwrap(),
        "original"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_history_limit_rejects_growth_without_blocking_replacement_or_back() {
    use turbo_surf_render::{AuxiliaryHostServices, AuxiliaryResourcePolicy};
    let server = fixture::Server::start().await;
    let services = AuxiliaryHostServices {
        policy: AuxiliaryResourcePolicy {
            max_history_entries: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut page = fixture::open(&server.origin, services).await;
    let target = fixture::child(&mut page).await;
    for path in ["/one", "/two"] {
        page.navigate_target(target, &server.url(path), "push")
            .await
            .unwrap();
        fixture::settle(&mut page).await;
    }
    assert!(page
        .navigate_target(target, &server.url("/rejected"), "push")
        .await
        .unwrap_err()
        .contains("history limit"));
    assert_eq!(server.requests().len(), 2);
    page.navigate_target(target, &server.url("/replacement"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    page.navigate_target(target, "", "back").await.unwrap();
    fixture::settle(&mut page).await;
    page.navigate_target(target, &server.url("/new-branch"), "push")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        page.eval_target_expression(target, "document.title", false, false)
            .await
            .unwrap(),
        "new-branch"
    );
    assert!(page
        .navigate_target(target, "", "forward")
        .await
        .unwrap_err()
        .contains("out of range"));
}
