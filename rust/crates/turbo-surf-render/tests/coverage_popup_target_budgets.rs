//! Target-local watchdog failures and surviving actor work use real native pages.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

async fn open() -> PageSession {
    PageSession::open(
        "<!doctype html><title>root</title><p>retained root</p>",
        "https://target-budget.test/",
        "",
        "",
        250,
    )
    .await
    .unwrap()
}

fn read(page: &mut PageSession, target: u64, source: &str) -> Value {
    serde_json::from_str(&page.read_target_engine(target, source).unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn a_target_engine_read_watchdog_preserves_reached_state_and_other_pages() {
    let mut page = open().await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    let error = page
        .read_target_engine(child, "globalThis.reachedRead=17;while(true){}")
        .unwrap_err();
    assert_eq!(error, "Target engine read exceeded session budget");
    assert_eq!(
        read(&mut page, root, "return [document.title,window.closed]"),
        json!(["root", false])
    );
    assert_eq!(
        read(
            &mut page,
            child,
            "return [reachedRead,document.title,window.closed]"
        ),
        json!([17, "original", false])
    );
    assert_eq!(read(&mut page, root, "return 6*7"), 42);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn a_target_authored_script_watchdog_rejects_without_poisoning_next_evaluation() {
    let mut page = open().await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    let error = page
        .eval_target_script(
            child,
            "document.title='reached before timeout';while(true){}",
        )
        .await
        .unwrap_err();
    assert_eq!(error, "Target script exceeded session budget");
    assert_eq!(
        read(&mut page, child, "return document.title"),
        "reached before timeout"
    );
    assert_eq!(
        read(&mut page, root, "return [document.title,window.closed]"),
        json!(["root", false])
    );
    page.eval_target_script(child, "document.title='recovered child';")
        .await
        .unwrap();
    assert_eq!(
        read(&mut page, child, "return document.title"),
        "recovered child"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn content_script_timeout_keeps_the_root_live_and_accepts_a_later_replacement() {
    let mut page = open().await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    let error = page
        .set_target_content(
            child,
            "<!doctype html><title>reached replacement</title><script>while(true){}</script>",
            "load",
            Some(100),
        )
        .await
        .unwrap_err();
    assert_eq!(error, "setContent timeout after 100ms");
    assert_eq!(
        read(&mut page, root, "return [document.title,window.closed]"),
        json!(["root", false])
    );
    let html = page
        .set_target_content(
            child,
            "<!doctype html><title>healthy replacement</title><p>fresh body</p>",
            "load",
            Some(5000),
        )
        .await
        .unwrap();
    assert!(html.contains("fresh body"));
    assert_eq!(
        read(
            &mut page,
            child,
            "return [document.title,document.readyState]"
        ),
        json!(["healthy replacement", "complete"])
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn polling_an_actor_after_root_close_services_its_actual_surviving_child() {
    let mut page = open().await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    assert_eq!(
        read(
            &mut page,
            child,
            "globalThis.childCounter=17;Promise.resolve().then(()=>childCounter++);return childCounter"
        ),
        17
    );
    assert!(page.close_target(root).unwrap());
    assert!(page.has_owned_pages());
    page.poll_owned_work().await.unwrap();
    assert_eq!(
        read(
            &mut page,
            child,
            "return [childCounter,document.title,window.closed]"
        ),
        json!([18, "original", false])
    );
    assert!(page
        .read_target_engine(root, "return document.title")
        .is_err());
    assert!(!page.close_target(child).unwrap());
    assert!(!page.has_owned_pages());
    assert_eq!(page.poll_owned_work().await.unwrap(), "");
    page.close();
}
