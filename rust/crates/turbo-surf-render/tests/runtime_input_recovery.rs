use turbo_surf_render::{NetworkEventLog, PageSession, PageSessionOptions};

#[tokio::test(flavor = "current_thread")]
async fn a_closed_host_event_log_rejects_session_open() {
    let log = NetworkEventLog::default();
    log.close();
    let result = PageSession::open_with_document_response_and_event_log(
        "<body>closed</body>",
        "https://entry.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        PageSessionOptions::default(),
        Some(log),
    )
    .await;
    assert_eq!(
        result.err().expect("closed event log must reject open"),
        "Page session closed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failed_input_cancels_its_run_without_poisoning_the_next_input() {
    let mut page = PageSession::open(
        "<input id=q><script>globalThis.keys=[];q.addEventListener('keydown',e=>keys.push([e.key,e.isTrusted]));</script>",
        "https://entry.test/", "", "", 5000,
    ).await.unwrap();
    for (steps, message) in [
        (serde_json::json!({}), "Input steps must be an array"),
        (
            serde_json::json!([{"focus":"#absent"}]),
            "Missing input selector #absent",
        ),
        (serde_json::json!([{}]), "Unsupported input step"),
    ] {
        let error = page.input(&steps).await.unwrap_err();
        assert!(error.contains(message), "{error}");
    }
    page.input(&serde_json::json!([{"focus":"#q"},{"press":"x"}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval_json_expression("keys").await.unwrap(),
        "[[\"x\",true]]"
    );
}
