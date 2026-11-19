use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn root_json_collector_keeps_authored_exceptions_and_remains_usable() {
    let mut page = PageSession::open("<body>ready", "https://collector.test/", "", "", 60_000)
        .await
        .unwrap();
    for (expression, message) in [
        (
            "({get value(){throw new TypeError('root getter detail')}})",
            "TypeError: root getter detail",
        ),
        (
            "Promise.resolve({get value(){throw 'root string detail'}})",
            "root string detail",
        ),
        ("undefined", "JSON serializable"),
    ] {
        let error = page.eval_json_expression(expression).await.unwrap_err();
        assert!(error.contains(message), "{error}");
        assert_eq!(
            page.eval_json_expression("({ready:42})").await.unwrap(),
            "{\"ready\":42}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn child_json_collector_keeps_its_error_without_poisoning_root_or_child() {
    let mut page = PageSession::open("<body>root", "https://collector.test/", "", "", 60_000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap().to_string();
    page.eval_expression("window.open('', 'collector');void 0")
        .await
        .unwrap();
    let child = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.source_page.as_deref() == Some(&root) && !info.closed)
        .unwrap()
        .target
        .parse()
        .unwrap();
    let error = page
        .eval_target_expression(
            child,
            "({get value(){throw new Error('child getter detail')}})",
            false,
            true,
        )
        .await
        .unwrap_err();
    assert!(error.contains("Error: child getter detail"), "{error}");
    assert_eq!(
        page.eval_target_expression(child, "({child:true})", false, true)
            .await
            .unwrap(),
        "{\"child\":true}"
    );
    assert_eq!(
        page.eval_json_expression("({root:true})").await.unwrap(),
        "{\"root\":true}"
    );
}
