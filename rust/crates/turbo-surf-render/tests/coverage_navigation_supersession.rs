//! A replacement navigation is distinguishable from actual target closure.
#[path = "support/popup_contract.rs"]
pub mod fixture;

#[tokio::test(flavor = "current_thread")]
async fn authored_replacements_interrupt_only_the_superseded_navigation() {
    for action in [
        "location.assign('/winner')",
        "location.replace('/winner')",
        "window.open('/winner', 'contract-child')",
    ] {
        let server = fixture::Server::start().await;
        let mut page = fixture::open(&server.origin, Default::default()).await;
        let child = fixture::child(&mut page).await;
        page.read_target_engine(
            child,
            &format!("Promise.resolve().then(()=>{{{action}}});return 'armed'"),
        )
        .unwrap();
        let error = page
            .navigate_target_with_options(
                child,
                &server.url("/must-not-load"),
                "push",
                r#"{"waitUntil":"load","timeoutMs":1000}"#,
            )
            .await
            .unwrap_err();
        assert_eq!(
            error, "Navigation interrupted by another navigation",
            "{action}"
        );
        fixture::settle(&mut page).await;
        assert!(
            !page
                .auxiliary_event_log()
                .info(&child.to_string())
                .unwrap()
                .closed
        );
        assert_eq!(
            page.read_target_engine(child, "return [location.pathname,document.title]")
                .unwrap(),
            r#"["/winner","winner"]"#,
            "{action}"
        );
        assert_eq!(
            server
                .requests()
                .iter()
                .map(|request| request.target.as_str())
                .collect::<Vec<_>>(),
            ["/winner"],
            "superseded lazy request must not reach HTTP: {action}"
        );
        page.close();
    }
}
