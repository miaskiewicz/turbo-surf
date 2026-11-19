//! Legacy session eval drains its actual root, not an unrelated Page in the same actor.
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use turbo_surf_render::{PageSession, PageSessionOptions, WindowTimerMode};

const BUDGET_MS: u64 = 5000;

struct WireServer {
    base: String,
    seen: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<()>>,
    closed: oneshot::Receiver<()>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for WireServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let mut chunk = [0; 4096];
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0);
        bytes.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(bytes).unwrap()
}
async fn serve() -> WireServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let (requested, seen) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let (closed_sender, closed) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.unwrap();
        assert!(request(&mut peer).await.starts_with("GET /peer "));
        let body = "<body>peer-document</body>";
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        peer.write_all(response.as_bytes()).await.unwrap();
        drop(peer);
        let (mut work, _) = listener.accept().await.unwrap();
        assert!(request(&mut work).await.starts_with("GET /work "));
        requested.send(()).unwrap();
        let mut released = std::pin::pin!(released);
        loop {
            tokio::select! {
                _ = &mut released => break,
                accepted = listener.accept() => {
                    let (mut own, _) = accepted.unwrap();
                    assert!(request(&mut own).await.starts_with("GET /own "));
                    own.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nroot-io!").await.unwrap();
                }
            }
        }
        drop(work);
        let _ = closed_sender.send(());
    });
    WireServer {
        base,
        seen,
        release: Some(release),
        closed,
        task,
    }
}
async fn start_peer_fetch(page: &mut PageSession, peer: u64, seen: &mut oneshot::Receiver<()>) {
    page.eval_target_expression(peer, "globalThis.peerSettled=false;globalThis.peerOutcome=fetch('/work').then(()=>{peerSettled=true;return 'resolved'},error=>{globalThis.peerFailure=error;peerSettled=true;return 'rejected'});void 0", false, false).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            tokio::select! {
                actual_request = &mut *seen => { actual_request.unwrap(); break; }
                turn = page.poll_background_work() => { turn.unwrap(); }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the peer must issue a real native HTTP request before root eval");
}
fn retained_pages(page: &PageSession, root: u64, peer: u64) -> bool {
    let log = page.auxiliary_event_log();
    log.root() == Some(root.to_string())
        && [root, peer].into_iter().all(|target| {
            log.pages()
                .iter()
                .any(|info| info.target == target.to_string() && !info.closed)
        })
}
async fn run_peer_guard(reject: bool) {
    run_peer_guard_program(reject, "globalThis.__RESULT='root-ready'", "root-ready").await;
}
async fn run_peer_guard_program(reject: bool, program: &str, expected: &str) {
    let mut server = serve().await;
    let mut page = PageSession::open_with_document_response_and_options(
        "<body>root-document</body>",
        &server.base,
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
    page.eval_expression("window.open('about:blank','independent-peer');void 0")
        .await
        .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.source_page.as_deref() == Some(&root.to_string()))
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    page.navigate_target(peer, &format!("{}peer", server.base), "push")
        .await
        .unwrap();
    start_peer_fetch(&mut page, peer, &mut server.seen).await;
    let log = page.network_event_log();
    assert_eq!(
        log.target_network_state(&peer.to_string())["pendingRequests"],
        1
    );
    assert_eq!(
        log.target_network_state(&root.to_string())["pendingRequests"],
        0
    );
    assert_eq!(
        page.read_target_engine(peer, "return peerSettled").unwrap(),
        "false"
    );
    assert_eq!(page.navigation_snapshot().1, server.base);
    assert!(retained_pages(&page, root, peer));
    if reject {
        server.release.take().unwrap().send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), &mut server.closed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            page.read_target_engine(peer, "return peerSettled").unwrap(),
            "false"
        );
    }
    let start = std::time::Instant::now();
    let result = page.eval(program).await;
    let elapsed = start.elapsed();
    let pending = log.target_network_state(&peer.to_string())["pendingRequests"].clone();
    let identities = retained_pages(&page, root, peer);
    let root_document = page.serialize_target(root).unwrap();
    let peer_document = page.serialize_target(peer).unwrap();
    assert_eq!(result.unwrap(), expected);
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "legacy root scalar eval consumed unrelated peer time: {elapsed:?}"
    );
    assert!(identities);
    assert!(root_document.contains("root-document"));
    assert!(peer_document.contains("peer-document"));
    if reject {
        let value: Value = serde_json::from_str(&page.eval_target_expression(peer, "peerOutcome.then(value=>JSON.stringify({value,name:peerFailure.name,own:peerFailure instanceof TypeError}))", false, false).await.unwrap()).unwrap();
        assert_eq!(
            value,
            json!({"value":"rejected","name":"TypeError","own":true})
        );
    } else {
        assert_eq!(
            pending, 1,
            "the peer native fetch must still be genuinely held"
        );
        assert_eq!(
            page.read_target_engine(peer, "return peerSettled").unwrap(),
            "false"
        );
    }
    page.close();
}
#[tokio::test(flavor = "current_thread")]
async fn legacy_root_eval_does_not_wait_for_a_same_actor_peer_native_fetch() {
    run_peer_guard(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn legacy_root_eval_keeps_a_physically_rejected_peer_task_in_its_own_realm() {
    run_peer_guard(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_root_void_crypto_stream_and_native_io_complete_with_a_held_peer() {
    run_peer_guard_program(false, r#"
      globalThis.__RESULT='pending';
      void(async()=>{
        const digest = new Uint8Array(await crypto.subtle.digest('SHA-256',new Uint8Array([1,2,3])));
        const source = new Blob([new Uint8Array([4,5,6])]).stream();
        const compressor=new CompressionStream('gzip'), decompressor=new DecompressionStream('gzip');
        const brands=[compressor instanceof CompressionStream,compressor.readable instanceof ReadableStream,
          compressor.writable instanceof WritableStream,decompressor instanceof DecompressionStream];
        let formatError=false, forgedError=false;
        try { new CompressionStream('unsupported'); }
        catch(error) { formatError=error instanceof TypeError && error.constructor.constructor('return typeof Deno')()==='undefined'; }
        try { Object.getOwnPropertyDescriptor(CompressionStream.prototype,'readable').get.call({}); }
        catch(error) { forgedError=error instanceof TypeError && error.constructor.constructor('return typeof Deno')()==='undefined'; }
        const decoded = source.pipeThrough(compressor).pipeThrough(decompressor);
        const bytes = new Uint8Array(await new Response(decoded).arrayBuffer());
        const own = await fetch('/own').then(response=>response.text());
        globalThis.__RESULT=JSON.stringify({digest:Array.from(digest).map(value=>value.toString(16).padStart(2,'0')).join(''),bytes:Array.from(bytes),own,brands,formatError,forgedError});
      })();
      void 0;
    "#, r#"{"digest":"039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81","bytes":[4,5,6],"own":"root-io!","brands":[true,true,true,true],"formatError":true,"forgedError":true}"#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_root_borrowed_crypto_keeps_its_real_peer_realm_and_explicit_dependency() {
    run_peer_guard_program(false, r#"
      globalThis.__RESULT='pending';
      const cryptoPeer=window.open('about:blank','crypto-peer');
      const promise=cryptoPeer.crypto.subtle.digest('SHA-256',new Uint8Array([1,2,3]));
      const brand=promise instanceof cryptoPeer.Promise && !(promise instanceof Promise);
      void(async()=>{const value=await promise;globalThis.__RESULT=String(brand)+':'+value.byteLength})();
      void 0;
    "#, "true:32").await;
}
