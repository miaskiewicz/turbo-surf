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
