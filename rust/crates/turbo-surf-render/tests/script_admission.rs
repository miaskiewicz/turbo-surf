use turbo_surf_render::{AuxiliaryHostServices, AuxiliaryResourcePolicy, PageSession};

#[tokio::test(flavor = "current_thread")]
async fn rejected_navigation_does_not_replace_the_last_actual_admission() {
    let mut page = PageSession::open_with_document_response_and_auxiliary_services(
        "<body>ready",
        "http://127.0.0.1:9/",
        "",
        "",
        60_000,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        Default::default(),
        "",
        None,
        AuxiliaryHostServices {
            policy: AuxiliaryResourcePolicy {
                max_navigation_url_bytes: 64,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    page.eval_expression("location.assign('/admitted');void 0")
        .await
        .unwrap();
    let admission = Some(("http://127.0.0.1:9/admitted".to_owned(), false));
    assert_eq!(page.script_nav_activation_snapshot(), admission);
    let rejected = format!("http://127.0.0.1:9/{}", "x".repeat(128));
    page.eval_expression(&format!(
        "location.replace({});void 0",
        serde_json::to_string(&rejected).unwrap()
    ))
    .await
    .unwrap();
    assert_eq!(page.script_nav_activation_snapshot(), admission);
}
