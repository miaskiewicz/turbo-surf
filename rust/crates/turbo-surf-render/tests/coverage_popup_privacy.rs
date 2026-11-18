//! Real popup privacy is observable on both the wire and retained documents.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

fn read(page: &mut PageSession, target: u64, source: &str) -> Value {
    serde_json::from_str(&page.read_target_engine(target, source).unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn noreferrer_popup_suppresses_wire_referrer_and_opener_without_harming_normal_popups() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval("globalThis.rootMarker=73;globalThis.plain=window.open('/plain','_blank');globalThis.plainReturned=plain!==null;")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    let log = page.auxiliary_event_log();
    let normal = log
        .pages()
        .into_iter()
        .find(|info| info.url == server.url("/plain") && !info.closed)
        .expect("normal popup must complete actual navigation");
    assert_eq!(normal.opener_page, Some(root.to_string()));
    assert_eq!(
        read(
            &mut page,
            root,
            "return [rootMarker,plainReturned,plain.opener===window]"
        ),
        json!([73, true, true])
    );
    assert_eq!(
        read(
            &mut page,
            normal.target.parse().unwrap(),
            "return [document.title,document.referrer,opener!==null,window.closed]"
        ),
        json!(["plain", server.url("/opener"), true, false])
    );

    page.eval("globalThis.privateReturned=window.open('/private','_blank','noreferrer')===null;")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    let private = log
        .pages()
        .into_iter()
        .find(|info| info.url == server.url("/private") && !info.closed)
        .expect("noreferrer still creates a real, completed auxiliary document");
    assert_ne!(private.target, normal.target);
    assert_eq!(private.opener_page, None);
    assert_eq!(
        read(
            &mut page,
            private.target.parse().unwrap(),
            "return [document.title,document.referrer,opener===null,window.closed]"
        ),
        json!(["private", "", true, false])
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].target, "/plain");
    assert_eq!(
        requests[0].headers.get("referer"),
        Some(&server.url("/opener"))
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].target, "/private");
    assert!(!requests[1].headers.contains_key("referer"));
    assert_eq!(
        read(
            &mut page,
            root,
            "return [rootMarker,privateReturned,plain.closed,plain.document.title,window.closed]"
        ),
        json!([73, true, false, "plain", false])
    );
    page.close();
}
