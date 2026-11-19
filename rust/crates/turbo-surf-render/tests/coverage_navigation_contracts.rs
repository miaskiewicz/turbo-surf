//! Native action receipts retain the actual completed navigation outcome and owner.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use turbo_surf_render::PageSession;

fn revision(page: &PageSession, target: u64) -> String {
    page.network_event_log()
        .target_network_state(&target.to_string())["navigationRevision"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn title(page: &mut PageSession, target: u64) -> Value {
    serde_json::from_str(
        &page
            .read_target_engine(target, "return document.title")
            .unwrap(),
    )
    .unwrap()
}

async fn rejecting_endpoint() -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rejected", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            let mut chunk = [0; 1024];
            let count = socket.read(&mut chunk).await.unwrap();
            assert_ne!(count, 0, "request closed before headers");
            bytes.extend_from_slice(&chunk[..count]);
            assert!(bytes.len() < 65_536);
        }
        // A physical request followed by EOF is the failure witness, not a fake result.
        drop(socket);
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_owned()
    });
    (url, task)
}

async fn complete_failed_root(page: &mut PageSession) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match page.poll_root_navigation_work().await {
                Ok(()) => assert!(
                    page.has_pending_auxiliary_work(),
                    "rejected own request must not silently complete"
                ),
                Err(error) => break error,
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("physical rejected navigation must report within the session budget")
}

#[tokio::test(flavor = "current_thread")]
async fn completed_navigation_failure_is_delivered_only_to_its_matching_action_revision() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    let baseline = revision(&page, root);
    let (url, rejected) = rejecting_endpoint().await;
    let source = format!(
        "location.assign({});globalThis.__RESULT='queued';",
        serde_json::to_string(&url).unwrap()
    );
    assert_eq!(
        page.eval_target_engine(root, &source, false).await.unwrap(),
        "queued"
    );
    let admitted = revision(&page, root);
    assert_ne!(admitted, baseline);
    let failure = complete_failed_root(&mut page).await;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), rejected)
            .await
            .unwrap()
            .unwrap(),
        "GET /rejected HTTP/1.1"
    );
    assert!(
        !page.has_pending_auxiliary_work(),
        "failure must already be terminal before the action receipt"
    );
    assert_eq!(title(&mut page, root), "opener");
    assert_eq!(title(&mut page, peer), "original");
    let outcome: Value = serde_json::from_str(
        &page
            .dispatch_target_engine_since(root, "", Some(&baseline))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(outcome["status"], "navigation-failed");
    assert_eq!(outcome["error"], failure);
    let current: Value = serde_json::from_str(
        &page
            .dispatch_target_engine_since(root, "", Some(&admitted))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        current,
        json!({"status":"released"}),
        "no new signal must not inherit an earlier error"
    );
    assert!(
        server.requests().is_empty(),
        "receipt delivery and passive owner reads do not refetch"
    );
    page.dispatch_target_engine(root, "location.assign('/healthy');")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(title(&mut page, root), "healthy");
    assert_eq!(title(&mut page, peer), "original");
    assert_eq!(page.auxiliary_root_id().unwrap(), root);
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.clone())
            .collect::<Vec<_>>(),
        ["/healthy"]
    );
    assert_ne!(revision(&page, root), admitted);
}

#[tokio::test(flavor = "current_thread")]
async fn completed_driver_failure_remains_an_error_for_its_matching_action_revision() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    page.configure_target_bootstrap(
        root,
        turbo_surf_render::AuxiliaryBootstrap {
            engine_init_script: "throw new Error('driver-contract-fault')".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let baseline = revision(&page, root);
    page.eval_target_engine(root, "location.assign('/driver-fault');", false)
        .await
        .unwrap();
    let admitted = revision(&page, root);
    assert_ne!(admitted, baseline);
    let failure = complete_failed_root(&mut page).await;
    assert!(failure.contains("driver bootstrap"), "{failure}");
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.clone())
            .collect::<Vec<_>>(),
        ["/driver-fault"]
    );
    assert!(!page.has_pending_auxiliary_work());
    assert_eq!(revision(&page, root), admitted);
    let error = page
        .dispatch_target_engine_since(root, "", Some(&baseline))
        .await
        .unwrap_err();
    assert_eq!(
        error, failure,
        "a completed driver failure must not become navigation-failed or released"
    );
    let no_signal: Value = serde_json::from_str(
        &page
            .dispatch_target_engine_since(root, "", Some(&admitted))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(no_signal, json!({"status":"released"}));
    page.configure_target_bootstrap(root, Default::default())
        .unwrap();
    page.dispatch_target_engine(root, "location.assign('/recovered-driver');")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(title(&mut page, root), "recovered-driver");
    assert_eq!(title(&mut page, peer), "original");
    assert_eq!(page.auxiliary_root_id().unwrap(), root);
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|request| request.target.clone())
            .collect::<Vec<_>>(),
        ["/driver-fault", "/recovered-driver"]
    );
}
