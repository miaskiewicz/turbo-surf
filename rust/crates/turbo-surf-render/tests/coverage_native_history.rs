#[path = "support/popup_contract.rs"]
pub mod fixture;
use turbo_surf_render::{AuxiliaryDriverHistory, AuxiliaryHostServices, PageSession};

async fn seeded(entries: Vec<String>, index: usize) -> Result<PageSession, String> {
    PageSession::open_with_document_response_and_auxiliary_services(
        "<!doctype html><title>seeded</title>",
        "https://history.test/opener",
        "",
        "",
        5_000,
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
            root_history: Some(AuxiliaryDriverHistory { entries, index }),
            ..Default::default()
        },
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn driver_history_seed_validates_selected_document_and_each_url() {
    for (entries, index, message) in [
        (vec![], 0, "actual initial document"),
        (
            vec!["https://history.test/opener".into()],
            1,
            "actual initial document",
        ),
        (
            vec!["https://history.test/other".into()],
            0,
            "actual initial document",
        ),
        (
            vec!["not a URL".into(), "https://history.test/opener".into()],
            1,
            "invalid URL",
        ),
    ] {
        let error = match seeded(entries, index).await {
            Ok(page) => {
                page.close();
                panic!("invalid driver seed admitted")
            }
            Err(error) => error,
        };
        assert!(error.contains(message), "{error}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn driver_seed_preserves_raw_rows_and_current_document_state() {
    let mut page = seeded(
        vec![
            "https://history.test/before".into(),
            "https://history.test/opener".into(),
            "https://history.test/after".into(),
        ],
        1,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let state = page
        .eval_target_expression(
            root,
            "[history.length,history.state,location.pathname]",
            false,
            true,
        )
        .await
        .unwrap();
    assert_eq!(state, "[3,null,\"/opener\"]");
    page.eval_target_expression(
        root,
        "history.replaceState({owned:true},'', '/owned')",
        false,
        false,
    )
    .await
    .unwrap();
    let state = page
        .eval_target_expression(
            root,
            "[history.length,history.state.owned,location.pathname]",
            false,
            true,
        )
        .await
        .unwrap();
    assert_eq!(state, "[3,true,\"/owned\"]");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_driver_traversal_keeps_selected_row_and_peer_alive() {
    let mut page = fixture::open("https://history.test", Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    page.eval_target_expression(root,
        "history.pushState({n:1},'', '/one');history.pushState({n:2},'', '/two');navigation.addEventListener('navigate', e=>e.preventDefault())",
        false, false).await.unwrap();
    let error = page.navigate_target(root, "", "back").await.unwrap_err();
    assert!(error.contains("canceled"), "{error}");
    let state = page
        .eval_target_expression(root, "[history.state.n,location.pathname]", false, true)
        .await
        .unwrap();
    assert_eq!(state, "[2,\"/two\"]");
    assert_eq!(
        page.eval_target_expression(peer, "document.title", false, false)
            .await
            .unwrap(),
        "original"
    );
    page.close();
}
