#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::{AuxiliaryBootstrap, AuxiliaryHostServices, AuxiliaryResourcePolicy};

#[tokio::test(flavor = "current_thread")]
async fn active_and_lifetime_limits_preserve_named_reuse_and_release_closed_pages() {
    let services = AuxiliaryHostServices {
        policy: AuxiliaryResourcePolicy {
            max_pages: 1,
            max_creations: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut page = fixture::open("https://popup.test", services).await;
    let actual = page
        .eval_json_expression(
            r#"(() => {
        const first=window.open('', 'reused');
        const blocked=window.open('', 'blocked');
        const reused=window.open('', 'reused');
        const initial=[first!==null,blocked===null,reused===first];
        first.close();
        const next=window.open('', 'next');
        initial.push(next!==null);
        next.close();
        initial.push(window.open('', 'exhausted')===null);
        return initial;
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&actual).unwrap(),
        json!([true, true, true, true, true])
    );
    assert_eq!(
        page.auxiliary_event_log()
            .pages()
            .iter()
            .filter(|info| !info.closed)
            .count(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_context_init_rolls_back_active_admission_and_preserves_the_opener() {
    let services = AuxiliaryHostServices {
        bootstrap: AuxiliaryBootstrap {
            init_script: "throw new Error('owned-init-fault')".into(),
            ..Default::default()
        },
        policy: AuxiliaryResourcePolicy {
            max_pages: 1,
            max_creations: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut page = fixture::open("https://popup.test", services).await;
    let error = page.eval_expression("(()=>{try{window.open('', 'bad');return 'not rejected'}catch(error){return error.message}})()").await.unwrap();
    assert!(error.contains("owned-init-fault"), "{error}");
    assert_eq!(
        page.auxiliary_event_log()
            .pages()
            .iter()
            .filter(|info| !info.closed)
            .count(),
        1
    );
    page.configure_auxiliary_bootstrap(AuxiliaryBootstrap {
        init_script: "globalThis.contextValue='recovered'".into(),
        ..Default::default()
    })
    .unwrap();
    let target = fixture::child(&mut page).await;
    assert_eq!(
        page.eval_target_expression(target, "contextValue", false, false)
            .await
            .unwrap(),
        "recovered"
    );
    assert_eq!(
        page.eval_expression("document.title").await.unwrap(),
        "opener"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn target_reads_content_viewport_and_retirement_remain_independent() {
    let mut page = fixture::open("https://popup.test", Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let target = fixture::child(&mut page).await;
    assert!(page.target_is_primary(root).unwrap());
    assert!(!page.target_is_primary(target).unwrap());
    let html = page.serialize_target_for_driver(target).unwrap();
    assert!(html.contains("original"));
    assert_eq!(
        page.read_target_engine(target, "return document.title")
            .unwrap(),
        "\"original\""
    );
    assert!(page
        .read_target_engine(target, "return Promise.resolve(1)")
        .unwrap_err()
        .contains("synchronous"));
    assert!(page.read_target_engine(target, "return 1n").is_err());
    assert_eq!(
        page.eval_target_expression(target, "({answer:42})", false, true)
            .await
            .unwrap(),
        "{\"answer\":42}"
    );
    let scalar: Value = serde_json::from_str(
        &page
            .eval_target_expression_tracked(target, "document.title")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(scalar["value"], "original");
    assert_eq!(scalar["needsSync"], false);
    let mutation: Value = serde_json::from_str(
        &page
            .eval_target_expression_tracked(target, "document.title='edited'")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mutation["needsSync"], true);
    page.resize_target(target, 400, 300).await.unwrap();
    let size = page
        .eval_target_expression(target, "[innerWidth,innerHeight]", false, true)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&size).unwrap(),
        json!([400, 300])
    );
    let html = page
        .set_target_content(
            target,
            "<!doctype html><title>replacement</title><p>new child</p>",
            "load",
            Some(5_000),
        )
        .await
        .unwrap();
    assert!(html.contains("new child"));
    assert_eq!(
        page.eval_expression("document.title").await.unwrap(),
        "opener"
    );
    assert!(page.close_target(target).unwrap());
    assert!(page.close_target(target).unwrap());
    assert!(page.serialize_target(target).is_err());
    assert!(page.resize_target(target, 1, 1).await.is_err());
    assert!(page
        .configure_target_bootstrap(target, Default::default())
        .is_err());
    assert!(!page.close_target(root).unwrap());
    assert!(!page.has_owned_pages());
    assert_eq!(page.poll_owned_work().await.unwrap(), "");
}

#[tokio::test(flavor = "current_thread")]
async fn overlong_guest_navigation_is_rejected_before_page_allocation_or_http() {
    let server = fixture::Server::start().await;
    let services = AuxiliaryHostServices {
        policy: AuxiliaryResourcePolicy {
            max_navigation_url_bytes: 16,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut page = fixture::open(&server.origin, services).await;
    let url = serde_json::to_string(&server.url("/must-not-load")).unwrap();
    assert_eq!(
        page.eval_expression(&format!("window.open({url}, '_blank') === null"))
            .await
            .unwrap(),
        "true"
    );
    assert_eq!(page.auxiliary_event_log().pages().len(), 1);
    assert!(server.requests().is_empty());
    assert!(!page.has_pending_auxiliary_work());
    assert_eq!(
        page.eval_expression("document.title").await.unwrap(),
        "opener"
    );
}
