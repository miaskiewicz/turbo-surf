//! A frame whose environment initialization fails is rolled back: its realm roots
//! are released, nothing is queued for it, and the same iframe host can retry.
//! Runs on the stock SDK as well as the native engine.

#[tokio::test(flavor = "current_thread")]
async fn failed_replacement_initialization_releases_roots_and_same_host_can_retry() {
    let mut page =
        turbo_surf_render::PageSession::open("<body>", "https://rollback.test/", "", "", 10000)
            .await
            .unwrap();
    page.fail_next_frame_initialization_for_testing();
    assert_eq!(page.eval_expression("globalThis.retryFrame=document.createElement('iframe');document.body.append(retryFrame);'attempted'").await.unwrap(),"attempted");
    assert!(page.take_frame_initialization_failure_for_testing());
    assert!(!page.take_frame_initialization_failure_for_testing());
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing(),
        serde_json::json!({"hostContexts":0,"retiredHostContexts":0,"queuedWindowMessages":0,"queuedWindowBytes":0})
    );
    assert_eq!(
        page.eval_expression("String(retryFrame.contentWindow!==null)")
            .await
            .unwrap(),
        "true"
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        1
    );
    page.eval_expression("retryFrame.remove();'removed'")
        .await
        .unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn failed_later_frame_navigation_can_recover_on_the_same_host() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=retryFrame></iframe>",
        "https://rollback.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        1
    );
    page.eval_expression("globalThis.retryFrame=document.getElementById('retryFrame');'ready'")
        .await
        .unwrap();
    page.fail_next_frame_initialization_for_testing();
    page.eval_expression("retryFrame.srcdoc='<body>failed';'attempted'")
        .await
        .unwrap();
    assert!(page.take_frame_initialization_failure_for_testing());
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    page.eval_expression("retryFrame.srcdoc='<body>recovered';'retried'")
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression("retryFrame.contentDocument.body.textContent")
            .await
            .unwrap(),
        "recovered"
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        1
    );
    page.eval_expression("retryFrame.remove();'removed'")
        .await
        .unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    page.close();
}
