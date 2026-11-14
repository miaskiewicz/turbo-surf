//! Authored session documents never run in the infrastructure realm.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn initial_document_is_an_actual_non_primary_guest_root() {
    let mut page = PageSession::open_with_init_script(
        "<body><script>globalThis.authoredRuns=(globalThis.authoredRuns||0)+1</script></body>",
        "https://guest.test/first",
        "",
        "",
        5000,
        "globalThis.initializedBeforeAuthored=typeof authoredRuns==='undefined'",
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    assert!(!page.target_is_primary(root).unwrap());
    assert_eq!(
        page.eval_engine_json_expression("__tsNs.isAuxiliary===true")
            .await
            .unwrap(),
        "true"
    );
    assert_eq!(
        page.eval_json_expression(
            "[authoredRuns,initializedBeforeAuthored,window===top,opener===null,typeof Deno]"
        )
        .await
        .unwrap(),
        "[1,true,true,true,\"undefined\"]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn closed_guest_root_never_falls_back_to_infrastructure() {
    let mut page = PageSession::open("<body>guest</body>", "https://guest.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.close_target(root).unwrap();
    assert!(page.read_engine("return location.href").is_err());
    assert!(page.eval_expression("location.href").await.is_err());
    assert!(page.eval("globalThis.infraTouched=true").await.is_err());
    assert!(page.collect_engine("return location.href").await.is_err());
    assert!(page.set_viewport(640, 480).await.is_err());
    assert!(page.serialize_checked().is_err());
    assert!(page.serialize_for_driver_checked().is_err());
    assert!(page.serialize().is_empty());
}
