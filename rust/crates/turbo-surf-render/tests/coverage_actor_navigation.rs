//! Public actor contracts: real document owners, driver snapshots, and host navigation.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use std::time::Duration;
use turbo_surf_render::{
    AuxiliaryBootstrap, AuxiliaryHostServices, AuxiliaryResourcePolicy, PageSession,
};

fn value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap()
}

async fn current(page: &mut PageSession, target: u64, expression: &str) -> Value {
    value(
        &page
            .eval_target_expression(target, expression, false, true)
            .await
            .unwrap(),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn actor_event_waiters_follow_actual_creation_and_actor_close() {
    let mut page = fixture::open("https://actor.test", Default::default()).await;
    let log = page.auxiliary_event_log();
    log.take();
    let revision = log.revision();
    let waiter = log.wait_for_revision(revision);
    tokio::pin!(waiter);
    assert!(tokio::time::timeout(Duration::from_millis(10), &mut waiter)
        .await
        .is_err());
    let child = fixture::child(&mut page).await;
    assert!(!tokio::time::timeout(Duration::from_secs(1), &mut waiter)
        .await
        .unwrap());
    assert!(log.revision() > revision);
    assert!(!log.wait().await);
    assert!(page.close_target(child).unwrap());
    page.close();
    assert!(log.wait().await);
    assert!(log.is_closed());
}

#[tokio::test(flavor = "current_thread")]
async fn driver_snapshots_notice_real_frame_and_cookie_mutations() {
    let mut page = fixture::open("https://actor.test", Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval_target_script(root, "globalThis.savedDocument=document;document.cookie='actor=one';globalThis.frameLoaded=new Promise(resolve=>{const frame=document.createElement('iframe');frame.onload=()=>{if(frame.contentDocument?.body?.textContent==='first')resolve(true)};frame.srcdoc='<p>first</p>';document.body.append(frame)});").await.unwrap();
    assert_eq!(current(&mut page, root, "frameLoaded").await, true);
    assert!(page
        .serialize_target_for_driver(root)
        .unwrap()
        .contains("iframe"));
    let cookies = page.cookies_target_for_driver(root).unwrap();
    assert!(cookies.contains("actor"));
    assert!(cookies.contains("one"));
    let unchanged = value(
        &page
            .eval_target_expression_tracked(root, "document===savedDocument")
            .await
            .unwrap(),
    );
    assert_eq!(unchanged["value"], "true");
    assert_eq!(unchanged["needsSync"], false);
    let changed = value(
        &page
            .eval_target_expression_tracked(
                root,
                "document.querySelector('iframe').contentDocument.body.textContent='second';true",
            )
            .await
            .unwrap(),
    );
    assert_eq!(changed["value"], "true");
    assert_eq!(changed["needsSync"], true);
    page.serialize_target_for_driver(root).unwrap();
    page.set_target_content(
        root,
        "<body><p>interactive replacement</p></body>",
        "domcontentloaded",
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        value(
            &page
                .read_target_engine(
                    root,
                    "return [document.readyState,document.body.textContent]"
                )
                .unwrap()
        ),
        json!(["interactive", "interactive replacement"])
    );
    page.set_target_content(
        root,
        "<body>complete replacement</body>",
        "networkidle",
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        current(
            &mut page,
            root,
            "[document.readyState,document.body.textContent]"
        )
        .await,
        json!(["complete", "complete replacement"])
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn navigation_init_exceptions_report_the_real_target_and_allow_reload() {
    let server = fixture::Server::start().await;
    let services = AuxiliaryHostServices {
        root_bootstrap: AuxiliaryBootstrap {
            init_script: "globalThis.contextValue='before-parser';if(location.pathname==='/init-failure')throw new Error('owned-init-failure');".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut page = fixture::open(&server.origin, services).await;
    let root = page.auxiliary_root_id().unwrap();
    page.navigate_target_with_options(
        root,
        &server.url("/init-failure"),
        "push",
        r#"{"waitUntil":"load"}"#,
    )
    .await
    .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        current(
            &mut page,
            root,
            "[document.title,bootObservation[0],window===top]"
        )
        .await,
        json!(["init-failure", "before-parser", true])
    );
    let events: Value = value(&page.network_event_log().drain());
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["event"] == "pageerror"
            && event["message"]
                .as_str()
                .is_some_and(|message| message.contains("owned-init-failure"))));
    page.navigate_target_with_options(
        root,
        &server.url("/healthy"),
        "replace",
        r#"{"waitUntil":"load","reload":true}"#,
    )
    .await
    .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        current(&mut page, root, "[document.title,bootObservation[0]]").await,
        json!(["healthy", "before-parser"])
    );
    assert_eq!(page.auxiliary_root_id().unwrap(), root);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn cross_origin_popup_location_writes_resolve_against_the_genuine_source() {
    let source = fixture::Server::start().await;
    let destination = fixture::Server::start().await;
    let mut page = fixture::open(&source.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    page.navigate_target_with_options(
        child,
        &destination.url("/foreign"),
        "push",
        r#"{"waitUntil":"load"}"#,
    )
    .await
    .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        current(&mut page, child, "[location.href,document.title]").await,
        json!([destination.url("/foreign"), "foreign"])
    );
    page.eval_target_script(root, "globalThis.child.location='/from-opener';")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        current(&mut page, child, "[location.href,document.title]").await,
        json!([source.url("/from-opener"), "from-opener"])
    );
    assert!(source
        .requests()
        .iter()
        .any(|request| request.target == "/from-opener"));
    assert!(!destination
        .requests()
        .iter()
        .any(|request| request.target == "/from-opener"));
    assert_eq!(current(&mut page, root, "document.title").await, "opener");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_long_navigation_preserves_the_real_document_and_next_admission() {
    let server = fixture::Server::start().await;
    let services = AuxiliaryHostServices {
        policy: AuxiliaryResourcePolicy {
            max_navigation_url_bytes: 128,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut page = fixture::open(&server.origin, services).await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval_target_script(
        root,
        "globalThis.keptDocument=document;globalThis.counter=17;",
    )
    .await
    .unwrap();
    let long = server.url(&format!("/{}", "x".repeat(256)));
    let error = page.navigate_target(root, &long, "push").await.unwrap_err();
    assert_eq!(error, "Native target navigation URL budget exceeded");
    assert!(server.requests().is_empty());
    assert_eq!(
        current(&mut page, root, "[document===keptDocument,++counter]").await,
        json!([true, 18])
    );
    page.navigate_target_with_options(
        root,
        &server.url("/accepted"),
        "push",
        r#"{"waitUntil":"load"}"#,
    )
    .await
    .unwrap();
    assert_eq!(current(&mut page, root, "document.title").await, "accepted");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn public_history_back_navigates_only_its_actual_target() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    page.eval_target_script(child, "globalThis.peerCounter=17;")
        .await
        .unwrap();
    for path in ["/one", "/two"] {
        page.navigate_target_with_options(
            root,
            &server.url(path),
            "push",
            r#"{"waitUntil":"load"}"#,
        )
        .await
        .unwrap();
    }
    page.eval_target_script(root, "history.back();")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        current(&mut page, root, "[location.href,document.title]").await,
        json!([server.url("/one"), "one"])
    );
    assert_eq!(current(&mut page, child, "++peerCounter").await, 18);
    assert!(
        !page
            .auxiliary_event_log()
            .info(&child.to_string())
            .unwrap()
            .closed
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn closed_target_rejection_and_last_actor_close_preserve_a_live_peer() {
    let mut page = fixture::open("https://actor.test", Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    assert!(page.close_target(root).unwrap());
    assert!(page.close_target(root).unwrap());
    assert!(page
        .read_target_engine(root, "return document.title")
        .is_err());
    assert!(page.close_target(u64::MAX).is_err());
    page.eval_target_script(child, "document.title='surviving peer';")
        .await
        .unwrap();
    assert_eq!(
        current(&mut page, child, "document.title").await,
        "surviving peer"
    );
    assert!(!page.close_target(child).unwrap());
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn cross_origin_popup_navigation_uses_captured_base_without_guest_getters() {
    let source = fixture::Server::start().await;
    let destination = fixture::Server::start().await;
    let mut page = fixture::open(&source.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let child = fixture::child(&mut page).await;
    page.navigate_target_with_options(
        child,
        &destination.url("/foreign"),
        "push",
        r#"{"waitUntil":"load"}"#,
    )
    .await
    .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        current(&mut page, child, "location.href").await,
        destination.url("/foreign")
    );
    page.eval_target_script(root, "const base=document.createElement('base');base.href='/source-directory/';document.head.append(base);globalThis.guestGetterCalls=0;for(const key of ['baseURI','URL'])Object.defineProperty(document,key,{configurable:true,get(){guestGetterCalls++;throw new Error('guest URL getter must not execute')}});globalThis.child.location='relative-result';").await.unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        current(&mut page, child, "[location.href,document.title]").await,
        json!([
            source.url("/source-directory/relative-result"),
            "source-directory/relative-result"
        ])
    );
    assert_eq!(current(&mut page, root, "guestGetterCalls").await, 0);
    assert!(source
        .requests()
        .iter()
        .any(|request| request.target == "/source-directory/relative-result"));
    assert!(!destination
        .requests()
        .iter()
        .any(|request| request.target.contains("relative-result")));
    page.close();
}
