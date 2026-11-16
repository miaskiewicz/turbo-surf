//! Lifecycle boundaries for native async ownership in a shared Page actor.
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use turbo_surf_render::{PageSession, PageSessionOptions, WindowTimerMode};

const BUDGET_MS: u64 = 5000;

struct HeldPeer {
    origin: String,
    seen: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Vec<String>>,
}
impl Drop for HeldPeer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let mut chunk = [0; 1024];
        let count = socket.read(&mut chunk).await.unwrap();
        assert!(count > 0 && bytes.len() + count <= 8192);
        bytes.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(bytes)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned()
}
async fn response(socket: &mut TcpStream, body: &str) {
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
    socket.write_all(response.as_bytes()).await.unwrap();
}
async fn serve() -> HeldPeer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (requested, seen) = oneshot::channel();
    let (release, mut released) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.unwrap();
        assert_eq!(request(&mut peer).await, "/peer");
        response(&mut peer, "<body>peer-document</body>").await;
        drop(peer);
        let (mut held, _) = listener.accept().await.unwrap();
        assert_eq!(request(&mut held).await, "/work");
        requested.send(()).unwrap();
        let mut requests = vec!["/peer".to_owned(), "/work".to_owned()];
        loop {
            tokio::select! {
                result=&mut released=>{result.unwrap();break;}
                accepted=listener.accept()=>{
                    let (mut next,_)=accepted.unwrap();
                    let path=request(&mut next).await;
                    assert_eq!(path,"/replacement");
                    response(&mut next,"<body>replacement-document</body>").await;
                    requests.push(path);
                }
            }
        }
        response(&mut held, "peer-finished").await;
        requests
    });
    HeldPeer {
        origin,
        seen,
        release: Some(release),
        task,
    }
}
async fn actor() -> (PageSession, u64, u64, HeldPeer) {
    let mut wire = serve().await;
    let mut page = PageSession::open_with_document_response_and_options(
        "<body>root-document</body>",
        &format!("{}/root", wire.origin),
        "",
        "",
        BUDGET_MS,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.eval_expression("window.open('about:blank','lifecycle-peer');void 0")
        .await
        .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|entry| entry.source_page.as_deref() == Some(&root.to_string()))
        .unwrap()
        .target
        .parse()
        .unwrap();
    page.navigate_target(peer, &format!("{}/peer", wire.origin), "push")
        .await
        .unwrap();
    page.eval_target_expression(peer,"globalThis.peerMarker='preserved';globalThis.peerDone=false;globalThis.peerOutcome=fetch('/work').then(response=>response.text()).then(body=>{peerDone=true;return body});void 0",false,false).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            tokio::select! {
                result=&mut wire.seen=>{result.unwrap();break;}
                turn=page.poll_background_work()=>{turn.unwrap();}
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a real peer request must be held before the lifecycle action");
    assert_held(&mut page, peer);
    (page, root, peer, wire)
}
fn assert_held(page: &mut PageSession, peer: u64) {
    assert_eq!(
        page.network_event_log()
            .target_network_state(&peer.to_string())["pendingRequests"],
        1
    );
    assert_eq!(
        page.read_target_engine(peer, "return peerDone").unwrap(),
        "false"
    );
    assert!(page
        .serialize_target(peer)
        .unwrap()
        .contains("peer-document"));
}
async fn finish_peer(page: &mut PageSession, peer: u64, wire: &mut HeldPeer) {
    wire.release.take().unwrap().send(()).unwrap();
    assert_eq!(
        page.eval_target_expression(peer, "peerOutcome", false, false)
            .await
            .unwrap(),
        "peer-finished"
    );
    let actual: Value = serde_json::from_str(
        &page
            .eval_target_expression(
                peer,
                "JSON.stringify({marker:peerMarker,done:peerDone,old:typeof fromOldRoot})",
                false,
                false,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        actual,
        json!({"marker":"preserved","done":true,"old":"undefined"})
    );
    assert_eq!(
        page.network_event_log()
            .target_network_state(&peer.to_string())["pendingRequests"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pending_peer_does_not_keep_a_replaced_root_async_realm_alive() {
    let (mut page, root, peer, mut wire) = actor().await;
    page.eval_target_expression(
        root,
        "globalThis.oldRootPromise=new Promise(()=>{});globalThis.oldRootMarker={};void 0",
        false,
        false,
    )
    .await
    .unwrap();
    page.navigate_target(root, &format!("{}/replacement", wire.origin), "push")
        .await
        .unwrap();
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("replacement-document"));
    assert_held(&mut page, peer);
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let roles = page.context_registry_diagnostics_for_testing();
    assert_eq!(roles["guestRootKnown"], true, "{roles}");
    assert_eq!(roles["liveGuestRoot"], 1, "{roles}");
    assert_eq!(
        roles["liveOtherGuest"], 1,
        "only the live peer may remain: {roles}"
    );
    assert_eq!(roles["liveInfrastructure"], 1, "{roles}");
    assert!(roles["cleared"].as_u64().unwrap() >= 1, "{roles}");
    let old_marker: String = serde_json::from_str(
        &page
            .read_target_engine(root, "return typeof oldRootMarker")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(old_marker, "undefined");
    finish_peer(&mut page, peer, &mut wire).await;
    let requests = tokio::time::timeout(std::time::Duration::from_secs(3), &mut wire.task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests, vec!["/peer", "/work", "/replacement"]);
}

#[tokio::test(flavor = "current_thread")]
async fn closing_root_cancels_its_old_tasks_without_canceling_a_live_peer() {
    let (mut page, root, peer, mut wire) = actor().await;
    page.eval_target_expression(
        root,
        "setTimeout(()=>{window.open('','lifecycle-peer').fromOldRoot=true},0);void 0",
        false,
        false,
    )
    .await
    .unwrap();
    assert!(page.close_target(root).unwrap());
    assert!(page.serialize_target(root).is_err());
    assert_held(&mut page, peer);
    finish_peer(&mut page, peer, &mut wire).await;
    assert_eq!(
        page.eval_target_expression(
            peer,
            "new Promise(resolve=>setTimeout(()=>resolve('peer-recovered'),0))",
            false,
            false
        )
        .await
        .unwrap(),
        "peer-recovered"
    );
    assert!(
        page.auxiliary_event_log()
            .info(&root.to_string())
            .unwrap()
            .closed
    );
    let requests = tokio::time::timeout(std::time::Duration::from_secs(3), &mut wire.task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests, vec!["/peer", "/work"]);
}

async fn recover_after_eval_end(timeout: bool) {
    let (mut page, root, peer, mut wire) = actor().await;
    let expression="globalThis.evalStarted=true;new Promise(resolve=>setTimeout(()=>resolve('too-late'),60000))";
    if timeout {
        let error = page
            .eval_target_expression(root, expression, false, false)
            .await
            .unwrap_err();
        assert!(error.contains("budget"), "{error}");
    } else {
        let mut evaluation = Box::pin(page.eval_target_expression(root, expression, false, false));
        tokio::select! {
            biased;
            result=&mut evaluation=>panic!("held evaluation completed before drop: {result:?}"),
            _=tokio::time::sleep(std::time::Duration::from_millis(20))=>{}
        }
        drop(evaluation);
    }
    assert_eq!(
        page.read_target_engine(root, "return evalStarted").unwrap(),
        "true"
    );
    assert_held(&mut page, peer);
    assert_eq!(
        page.eval_target_expression(root, "'root-recovered'", false, false)
            .await
            .unwrap(),
        "root-recovered"
    );
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("root-document"));
    finish_peer(&mut page, peer, &mut wire).await;
    let requests = tokio::time::timeout(std::time::Duration::from_secs(3), &mut wire.task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests, vec!["/peer", "/work"]);
}
#[tokio::test(flavor = "current_thread")]
async fn dropping_a_pending_eval_preserves_the_actor_and_peer_work() {
    recover_after_eval_end(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn timed_out_eval_preserves_the_actor_and_peer_work() {
    recover_after_eval_end(true).await;
}
