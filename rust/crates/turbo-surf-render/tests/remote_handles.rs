use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn remote_values_are_private_live_and_disposable() {
    let mut page = PageSession::open("<b>ready</b>", "http://fixture.test/", "", "", 10_000)
        .await
        .unwrap();
    let id = page
        .eval_engine_expression(
            "__tsNs.remoteHandleRetain(globalThis, globalThis.shared={count:1})",
        )
        .await
        .unwrap();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 1);
    page.eval_expression("shared.count=2").await.unwrap();
    assert_eq!(
        page.eval_engine_expression(&format!("__tsNs.remoteHandleGet({id}).count"))
            .await
            .unwrap(),
        "2"
    );
    assert_eq!(
        page.eval_expression("typeof globalThis.__tsNs")
            .await
            .unwrap(),
        "undefined"
    );
    page.eval_engine(&format!("__tsNs.remoteHandleDispose({id});"))
        .await
        .unwrap();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 0);
    assert!(page
        .eval_engine_expression(&format!("__tsNs.remoteHandleGet({id})"))
        .await
        .unwrap_err()
        .contains("disposed"));
}

#[tokio::test(flavor = "current_thread")]
async fn remote_handle_index_does_not_root_detached_frame_realms() {
    let mut page = PageSession::open(
        "<iframe srcdoc='<b>child</b>'></iframe>",
        "http://fixture.test/",
        "",
        "",
        10_000,
    )
    .await
    .unwrap();
    let id = page.eval_engine_expression("__tsNs.remoteHandleRetain(document.querySelector('iframe').contentWindow, document.querySelector('iframe').contentWindow.document.body)").await.unwrap();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 1);
    page.eval_expression("document.querySelector('iframe').remove()")
        .await
        .unwrap();
    assert!(page
        .eval_engine_expression(&format!("__tsNs.remoteHandleGet({id})"))
        .await
        .unwrap_err()
        .contains("context"));
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 0);
}
