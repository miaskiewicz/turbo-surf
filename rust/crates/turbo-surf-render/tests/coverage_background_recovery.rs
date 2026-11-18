//! Public native turns recover after actual watchdog cancellation.
use turbo_surf_render::{
    BrowserStorage, DocumentResponseMetadata, PageSession, PageSessionOptions, WindowTimerMode,
};

#[tokio::test(flavor = "current_thread")]
async fn real_background_watchdog_preserves_reached_dom_and_allows_a_later_turn() {
    let mut page = PageSession::open_with_document_response_and_options(
        "<!doctype html><p id=state>before</p>",
        "https://background-recovery.test/",
        "",
        "",
        500,
        "",
        Default::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.read_engine("setTimeout(()=>{document.getElementById('state').textContent='reached';while(true){}},0);return true;").unwrap();
    let failure = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        page.poll_background_work(),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(failure, "Native background task exceeded execution budget");
    assert_eq!(
        page.read_engine("return document.getElementById('state').textContent;")
            .unwrap(),
        "\"reached\""
    );
    assert_eq!(page.auxiliary_root_id().unwrap(), root);
    page.read_engine("setTimeout(()=>{document.getElementById('state').textContent+=' recovered'},0);return true;").unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        page.poll_background_work(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        page.read_engine("return document.getElementById('state').textContent;")
            .unwrap(),
        "\"reached recovered\""
    );
    page.close();
}
