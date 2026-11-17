//! Real actor admissions preserve bounded queues and retire only their real owner.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::{AuxiliaryHostServices, AuxiliaryResourcePolicy, PageSession};

async fn open_with_one_pending(origin: &str) -> PageSession {
    fixture::open(
        origin,
        AuxiliaryHostServices {
            policy: AuxiliaryResourcePolicy {
                max_pending_navigations: 1,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await
}
fn admission(page: &PageSession, target: u64) -> Value {
    page.network_event_log()
        .target_network_state(&target.to_string())["navigationRevision"]
        .clone()
}
async fn queue(page: &mut PageSession, target: u64, path: &str) {
    let script = format!(
        "location.assign({});globalThis.__RESULT='queued';",
        serde_json::to_string(path).unwrap()
    );
    assert_eq!(
        page.eval_target_engine(target, &script, false)
            .await
            .unwrap(),
        "queued"
    );
}
async fn finish_pending(page: &mut PageSession) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while page.has_pending_auxiliary_work() {
            page.poll_owned_work().await.unwrap();
        }
    })
    .await
    .unwrap();
}

fn title(page: &mut PageSession, target: u64) -> Value {
    serde_json::from_str(
        &page
            .read_target_engine(target, "return document.title")
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn a_full_peer_queue_rejects_another_owner_without_spending_its_admission() {
    let server = fixture::Server::start().await;
    let mut page = open_with_one_pending(&server.origin).await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    let original_root = admission(&page, root);
    queue(&mut page, peer, "/peer-accepted").await;
    let accepted_peer = admission(&page, peer);
    queue(&mut page, root, "/root-rejected").await;
    assert_eq!(admission(&page, root), original_root);
    assert_eq!(admission(&page, peer), accepted_peer);
    assert!(
        server.requests().is_empty(),
        "queuing is not a native fetch"
    );
    assert_eq!(title(&mut page, root), "opener");
    finish_pending(&mut page).await;
    assert_eq!(title(&mut page, peer), "peer-accepted");
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.clone())
            .collect::<Vec<_>>(),
        ["/peer-accepted"]
    );
    queue(&mut page, root, "/root-after-capacity").await;
    finish_pending(&mut page).await;
    assert_eq!(title(&mut page, root), "root-after-capacity");
    assert_eq!(server.requests().len(), 2);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn same_owner_replacement_at_capacity_fetches_only_the_latest_real_admission() {
    let server = fixture::Server::start().await;
    let mut page = open_with_one_pending(&server.origin).await;
    let root = page.auxiliary_root_id().unwrap();
    queue(&mut page, root, "/superseded").await;
    let first = admission(&page, root);
    queue(&mut page, root, "/replacement").await;
    assert_ne!(admission(&page, root), first);
    assert!(server.requests().is_empty());
    finish_pending(&mut page).await;
    assert_eq!(title(&mut page, root), "replacement");
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.clone())
            .collect::<Vec<_>>(),
        ["/replacement"]
    );
    assert!(!page.has_pending_auxiliary_work());
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn closing_a_real_queued_peer_releases_capacity_without_fetch_or_replay() {
    let server = fixture::Server::start().await;
    let mut page = open_with_one_pending(&server.origin).await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    queue(&mut page, peer, "/closed-before-fetch").await;
    assert!(page.has_pending_auxiliary_work());
    assert!(
        page.close_target(peer).unwrap(),
        "root remains a real live actor"
    );
    assert!(
        page.auxiliary_event_log()
            .info(&peer.to_string())
            .unwrap()
            .closed
    );
    queue(&mut page, root, "/surviving-root").await;
    finish_pending(&mut page).await;
    assert_eq!(title(&mut page, root), "surviving-root");
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.clone())
            .collect::<Vec<_>>(),
        ["/surviving-root"]
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &page
                .read_target_engine(root, "return [window===top,window.closed]")
                .unwrap()
        )
        .unwrap(),
        json!([true, false])
    );
    page.close();
}
