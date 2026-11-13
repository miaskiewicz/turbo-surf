//! Network-idle evidence comes from native transport, not serialized event history.
use std::future::Future;
use std::pin::Pin;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use turbo_surf_render::PageSession;

struct HeldResponse {
    url: String,
    requested: oneshot::Receiver<()>,
    headers: Option<oneshot::Sender<()>>,
    started: oneshot::Receiver<()>,
    body: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for HeldResponse {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl HeldResponse {
    async fn new() -> Self {
        Self::with_body("text/plain", b"abc", b"def").await
    }
    async fn with_body(
        content_type: &'static str,
        first: &'static [u8],
        last: &'static [u8],
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hold", listener.local_addr().unwrap());
        let (requested, seen) = oneshot::channel();
        let (headers, wait_headers) = oneshot::channel();
        let (started, body_started) = oneshot::channel();
        let (body, wait_body) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 512];
            while !bytes.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let len = socket.read(&mut buffer).await.unwrap();
                assert_ne!(len, 0);
                bytes.extend_from_slice(&buffer[..len]);
                assert!(bytes.len() < 65536);
            }
            let _ = requested.send(());
            if wait_headers.await.is_err() {
                return;
            }
            let header = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", first.len() + last.len());
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(first).await.unwrap();
            let _ = started.send(());
            if wait_body.await.is_err() {
                return;
            }
            socket.write_all(last).await.unwrap();
        });
        Self {
            url,
            requested: seen,
            headers: Some(headers),
            started: body_started,
            body: Some(body),
            task,
        }
    }
}
async fn pending_until<F: Future<Output = Result<String, String>>>(
    future: &mut Pin<Box<F>>,
    signal: &mut oneshot::Receiver<()>,
) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::select! {
            result = future => panic!("transport unexpectedly completed: {result:?}"),
            result = signal => result.unwrap(),
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn held_headers_and_stream_body_keep_real_target_busy() {
    let mut server = HeldResponse::new().await;
    let mut page = PageSession::open("<body>root</body>", &server.url, "", "", 5000)
        .await
        .unwrap();
    let target = page.auxiliary_root_id().unwrap().to_string();
    let log = page.network_event_log();
    let before = log.target_network_state(&target);
    let source = format!("fetch({:?}).then(response=>response.text())", server.url);
    let mut pending =
        Box::pin(page.eval_target_expression(target.parse().unwrap(), &source, true, false));
    pending_until(&mut pending, &mut server.requested).await;
    assert_eq!(log.target_network_state(&target)["pendingRequests"], 1);
    assert_eq!(
        log.target_network_state(&target)["error"],
        serde_json::Value::Null
    );
    server.headers.take().unwrap().send(()).unwrap();
    pending_until(&mut pending, &mut server.started).await;
    assert_eq!(log.target_network_state(&target)["pendingRequests"], 1);
    server.body.take().unwrap().send(()).unwrap();
    assert_eq!(pending.await.unwrap(), "abcdef");
    let after = log.target_network_state(&target);
    assert_eq!(after["pendingRequests"], 0);
    assert_ne!(after["revision"], before["revision"]);
    page.close();
}

#[tokio::test]
async fn child_transport_never_changes_the_openers_quiet_revision() {
    let mut server = HeldResponse::new().await;
    let mut page = PageSession::open("<body>root</body>", &server.url, "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap().to_string();
    page.eval_expression("window.open('about:blank', 'idle-child'); 'opened'")
        .await
        .unwrap();
    let child = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|page| page.source_page.as_deref() == Some(root.as_str()))
        .unwrap()
        .target;
    let log = page.network_event_log();
    let root_quiet = log.target_network_state(&root);
    let source = format!("fetch({:?}).then(response=>response.text())", server.url);
    let mut pending =
        Box::pin(page.eval_target_expression(child.parse().unwrap(), &source, true, false));
    pending_until(&mut pending, &mut server.requested).await;
    assert_eq!(log.target_network_state(&child)["pendingRequests"], 1);
    assert_eq!(log.target_network_state(&root), root_quiet);
    server.headers.take().unwrap().send(()).unwrap();
    pending_until(&mut pending, &mut server.started).await;
    server.body.take().unwrap().send(()).unwrap();
    assert_eq!(pending.await.unwrap(), "abcdef");
    assert_eq!(log.target_network_state(&child)["pendingRequests"], 0);
    assert_eq!(log.target_network_state(&root), root_quiet);
    page.close();
}

#[tokio::test]
async fn native_module_loading_is_owned_before_response_headers() {
    let mut server =
        HeldResponse::with_body("application/javascript", b"export default ", b"7;").await;
    let mut page = PageSession::open("<body>root</body>", &server.url, "", "", 5000)
        .await
        .unwrap();
    let target = page.auxiliary_root_id().unwrap().to_string();
    let log = page.network_event_log();
    let source = format!("import({:?}).then(module=>module.default)", server.url);
    let mut pending =
        Box::pin(page.eval_target_expression(target.parse().unwrap(), &source, true, false));
    pending_until(&mut pending, &mut server.requested).await;
    assert_eq!(log.target_network_state(&target)["pendingRequests"], 1);
    assert_eq!(
        log.target_network_state(&target)["error"],
        serde_json::Value::Null
    );
    server.headers.take().unwrap().send(()).unwrap();
    pending_until(&mut pending, &mut server.started).await;
    assert_eq!(log.target_network_state(&target)["pendingRequests"], 1);
    server.body.take().unwrap().send(()).unwrap();
    assert_eq!(pending.await.unwrap(), "7");
    assert_eq!(log.target_network_state(&target)["pendingRequests"], 0);
    page.close();
}

#[tokio::test]
async fn native_image_loading_is_owned_until_transport_and_decode_finish() {
    let mut server = HeldResponse::new().await;
    let mut page = PageSession::open("<body>root</body>", &server.url, "", "", 5000)
        .await
        .unwrap();
    let target = page.auxiliary_root_id().unwrap().to_string();
    let log = page.network_event_log();
    let source = format!("new Promise(resolve=>{{const image=new Image();image.onload=()=>resolve('loaded');image.onerror=()=>resolve('decode-error');image.src={:?};}})", server.url);
    let mut pending =
        Box::pin(page.eval_target_expression(target.parse().unwrap(), &source, true, false));
    pending_until(&mut pending, &mut server.requested).await;
    assert_eq!(log.target_network_state(&target)["pendingRequests"], 1);
    assert_eq!(
        log.target_network_state(&target)["error"],
        serde_json::Value::Null
    );
    server.headers.take().unwrap().send(()).unwrap();
    pending_until(&mut pending, &mut server.started).await;
    assert_eq!(log.target_network_state(&target)["pendingRequests"], 1);
    server.body.take().unwrap().send(()).unwrap();
    assert_eq!(pending.await.unwrap(), "decode-error");
    assert_eq!(log.target_network_state(&target)["pendingRequests"], 0);
    page.close();
}

#[tokio::test]
async fn queued_native_form_is_busy_before_the_actor_starts_its_request() {
    let mut page = PageSession::open(
        "<form id='form' action='/next'></form>",
        "http://127.0.0.1:1/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let log = page.network_event_log();
    page.eval_target_expression(
        root,
        "document.querySelector('#form').requestSubmit(); 'queued'",
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        log.target_network_state(&root.to_string())["pendingNavigation"],
        true
    );
    page.close();
    assert_eq!(
        log.target_network_state(&root.to_string())["pendingNavigation"],
        false
    );
}

fn background_string(page: &mut PageSession, root: u64, source: &str) -> String {
    serde_json::from_str(&page.read_target_engine(root, source).unwrap()).unwrap()
}

async fn realtime_page(base: &str) -> PageSession {
    use turbo_surf_core::request_context::ClientHints;
    use turbo_surf_render::{
        BrowserStorage, DocumentResponseMetadata, PageSessionOptions, WindowTimerMode,
    };
    PageSession::open_with_document_response_and_options(
        "<body>root</body>",
        base,
        "",
        "",
        5000,
        "",
        ClientHints::default(),
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
    .unwrap()
}

#[tokio::test]
async fn background_turn_waits_for_real_timer_and_can_be_interrupted() {
    let mut page = realtime_page("http://127.0.0.1:1/").await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(
        root,
        "globalThis.done=false;setTimeout(()=>{globalThis.done=true},100);return '';",
    )
    .unwrap();
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(20),
        page.poll_background_work()
    )
    .await
    .is_err());
    assert_eq!(
        background_string(&mut page, root, "return String(globalThis.done)"),
        "false"
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        page.poll_background_work(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        background_string(&mut page, root, "return String(globalThis.done)"),
        "true"
    );
    page.close();
}

#[tokio::test]
async fn background_turns_progress_timer_fetch_and_body_continuation() {
    let mut server = HeldResponse::new().await;
    let mut page = realtime_page(&server.url).await;
    let root = page.auxiliary_root_id().unwrap();
    server.headers.take().unwrap().send(()).unwrap();
    server.body.take().unwrap().send(()).unwrap();
    let source = format!("globalThis.done='';setTimeout(()=>{{fetch({:?}).then(response=>response.text()).then(body=>{{globalThis.done=body}})}},100);return '';", server.url);
    page.read_target_engine(root, &source).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while background_string(&mut page, root, "return globalThis.done") != "abcdef" {
            page.poll_background_work().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(
        page.network_event_log()
            .target_network_state(&root.to_string())["pendingRequests"],
        0
    );
    page.close();
}

#[tokio::test]
async fn background_wait_never_advances_the_virtual_timer_clock() {
    let mut page = PageSession::open("<body>root</body>", "http://127.0.0.1:1/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(
        root,
        "globalThis.done=false;setTimeout(()=>{globalThis.done=true},10000);return '';",
    )
    .unwrap();
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(30),
        page.poll_background_work()
    )
    .await
    .is_err());
    assert_eq!(
        background_string(&mut page, root, "return String(globalThis.done)"),
        "false"
    );
    page.close();
}

#[tokio::test]
async fn held_child_io_does_not_prevent_the_openers_real_timer() {
    let mut server = HeldResponse::new().await;
    let mut page = realtime_page(&server.url).await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, "window.open('about:blank','held-child');return '';")
        .unwrap();
    let child = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|page| page.source_page.as_deref() == Some(root.to_string().as_str()))
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    page.read_target_engine(
        child,
        &format!(
            "fetch({:?}).then(response=>response.text());return '';",
            server.url
        ),
    )
    .unwrap();
    page.read_target_engine(
        root,
        "globalThis.done=false;setTimeout(()=>{globalThis.done=true},100);return '';",
    )
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while background_string(&mut page, root, "return String(globalThis.done)") != "true" {
            page.poll_background_work().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(server.requested.try_recv().is_ok());
    let log = page.network_event_log();
    assert_eq!(
        log.target_network_state(&child.to_string())["pendingRequests"],
        1
    );
    assert_eq!(
        log.target_network_state(&root.to_string())["pendingRequests"],
        0
    );
    page.close();
}
