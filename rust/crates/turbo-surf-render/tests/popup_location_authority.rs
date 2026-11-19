#[path = "support/popup_contract.rs"]
pub mod fixture;

#[tokio::test(flavor = "current_thread")]
async fn cross_origin_popup_can_replace_its_real_opener_using_its_own_origin() {
    let source = fixture::Server::start().await;
    let destination = fixture::Server::start().await;
    let mut page = fixture::open(&source.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    page.navigate_target(child, &destination.url("/cross"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    page.eval_target_expression(
        child,
        "(opener.location.replace('/returned'), true)",
        false,
        false,
    )
    .await
    .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        page.eval_target_expression(root, "document.title", false, false)
            .await
            .unwrap(),
        "returned"
    );
    assert_eq!(destination.requests().last().unwrap().target, "/returned");
    assert!(!source
        .requests()
        .iter()
        .any(|request| request.target == "/returned"));
}

#[tokio::test(flavor = "current_thread")]
async fn severed_opener_cannot_reuse_immutable_creation_metadata_for_navigation() {
    let source = fixture::Server::start().await;
    let destination = fixture::Server::start().await;
    let mut page = fixture::open(&source.origin, Default::default()).await;
    let child = fixture::child(&mut page).await;
    page.navigate_target(child, &destination.url("/cross"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    page.eval_target_expression(child, "(opener=null, true)", false, false)
        .await
        .unwrap();
    let failure = page.eval_expression("(() => {try {child.location.replace('/blocked');return 'accepted'}catch(error){return error.name}})()")
        .await.unwrap();
    assert_eq!(failure, "SecurityError");
    fixture::settle(&mut page).await;
    assert_eq!(
        page.eval_target_expression(child, "document.title", false, false)
            .await
            .unwrap(),
        "cross"
    );
    assert!(!source
        .requests()
        .iter()
        .chain(destination.requests().iter())
        .any(|request| request.target == "/blocked"));
}

#[tokio::test(flavor = "current_thread")]
async fn csp_sandbox_still_blocks_popup_navigation_of_its_real_opener() {
    let source = fixture::Server::start().await;
    let destination = fixture::Server::start().await;
    let mut page = fixture::open(&source.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    page.navigate_target(child, &destination.url("/sandbox-navigation"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    let failure = page.eval_target_expression(child, "(() => {try {opener.location.replace('/blocked');return 'accepted'}catch(error){return error.name}})()", false, false)
        .await.unwrap();
    assert_eq!(failure, "SecurityError");
    fixture::settle(&mut page).await;
    assert_eq!(
        page.eval_target_expression(root, "document.title", false, false)
            .await
            .unwrap(),
        "opener"
    );
    assert!(!source
        .requests()
        .iter()
        .chain(destination.requests().iter())
        .any(|request| request.target == "/blocked"));
}
