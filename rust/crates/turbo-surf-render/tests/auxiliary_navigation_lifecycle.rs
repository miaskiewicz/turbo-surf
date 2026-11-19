//! Public lifecycle operations retire a fetched document before its deferred initializer runs.
#[path = "support/popup_contract.rs"]
pub mod fixture;

use serde_json::{json, Value};
use std::time::Duration;
use turbo_surf_render::{AuxiliaryBootstrap, AuxiliaryHostServices, PageSession};

fn read(page: &mut PageSession, target: u64, script: &str) -> Value {
    serde_json::from_str(&page.read_target_engine(target, script).unwrap()).unwrap()
}

async fn installed_before_initialization(page: &mut PageSession, root: u64) {
    read(page, root, "location.href='/deferred'; return true");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            page.poll_root_navigation_work().await.unwrap();
            if read(page, root, "return document.title") == "deferred" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned HTTP document must be installed");
    assert_eq!(
        read(
            page,
            root,
            "return globalThis.deferredInitializerRan===true"
        ),
        false,
        "installation must yield before the guest initializer"
    );
    assert!(page.has_pending_auxiliary_work());
}

async fn exercise(operation: &str) {
    let server = fixture::Server::start().await;
    let services = AuxiliaryHostServices {
        root_bootstrap: AuxiliaryBootstrap {
            init_script: "if(location.pathname==='/deferred'){globalThis.deferredInitializerRan=true;console.log('retired-deferred-initializer');}".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut page = fixture::open(&server.origin, services).await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    read(&mut page, peer, "globalThis.peerCounter=40;return true");
    page.network_event_log().drain();
    installed_before_initialization(&mut page, root).await;
    match operation {
        "close" => assert!(page.close_target(root).unwrap()),
        "content" => {
            page.set_target_content(
                root,
                "<title>replacement</title><p>new content</p>",
                "load",
                None,
            )
            .await
            .unwrap();
        }
        "navigation" => {
            page.navigate_target_with_options(
                root,
                &server.url("/replacement"),
                "replace",
                r#"{"waitUntil":"load"}"#,
            )
            .await
            .unwrap();
        }
        _ => unreachable!(),
    }
    fixture::settle(&mut page).await;
    assert_eq!(
        page.eval_target_expression(peer, "[document.title,++peerCounter]", false, true)
            .await
            .map(|raw| serde_json::from_str::<Value>(&raw).unwrap())
            .unwrap(),
        json!(["original", 41]),
        "retirement must preserve a live peer's realm and document"
    );
    let events = page.network_event_log().drain();
    assert!(!events.contains("retired-deferred-initializer"), "{events}");
    if operation == "close" {
        assert!(
            page.auxiliary_event_log()
                .info(&root.to_string())
                .unwrap()
                .closed
        );
    } else {
        assert_eq!(
            read(&mut page, root, "return document.title"),
            "replacement"
        );
        assert_eq!(
            read(
                &mut page,
                root,
                "return globalThis.deferredInitializerRan===true"
            ),
            false
        );
    }
    assert!(!page.has_pending_auxiliary_work());
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn close_retires_installed_navigation_without_initializing_it_or_its_live_peer() {
    exercise("close").await;
}

#[tokio::test(flavor = "current_thread")]
async fn set_content_retires_installed_navigation_without_replaying_its_initializer() {
    exercise("content").await;
}

#[tokio::test(flavor = "current_thread")]
async fn superseding_navigation_retires_the_old_installed_initializer() {
    exercise("navigation").await;
}
