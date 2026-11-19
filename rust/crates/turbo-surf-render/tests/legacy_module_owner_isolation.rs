//! SOURCE-ONLY candidate: genuine registered modules and owned loopback wires.
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use turbo_surf_render::{PageSession, PageSessionOptions, WindowTimerMode};

const BUDGET_MS: u64 = 5000;
const PEER_MODULE: &str = r#"
globalThis.moduleStarted=true;
const value=await fetch('/held').then(response=>response.text());
globalThis.moduleFinished=true;
export const receipt={value,url:import.meta.url,document:document.URL};
"#;
const ROOT_MODULE: &str = r#"
const value=await fetch('/own').then(response=>response.text());
globalThis.__RESULT=JSON.stringify({value,url:import.meta.url,document:document.URL});
"#;

struct Server {
    base: String,
    held: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<bool>>,
    paths: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
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
async fn respond(socket: &mut TcpStream, mime: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await.unwrap();
}
async fn serve() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let (requested, held) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let requested = Arc::new(Mutex::new(Some(requested)));
    let released = Arc::new(Mutex::new(Some(released)));
    let paths = Arc::new(Mutex::new(Vec::new()));
    let receipts = paths.clone();
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (mut socket, _) = accepted.unwrap();
                    let requested = requested.clone();
                    let released = released.clone();
                    let receipts = receipts.clone();
                    tasks.spawn(async move {
                        let text = request(&mut socket).await;
                        let path = text.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_owned();
                        receipts.lock().unwrap().push(path.clone());
                        match path.as_str() {
                            "/peer" => respond(&mut socket,"text/html","<body>peer-document</body>").await,
                            "/peer-module.mjs" => respond(&mut socket,"application/javascript",PEER_MODULE).await,
                            "/root-module.mjs" => respond(&mut socket,"application/javascript",ROOT_MODULE).await,
                            "/own" => respond(&mut socket,"text/plain","root-wire!").await,
                            "/held" => {
                                requested.lock().unwrap().take().unwrap().send(()).unwrap();
                                let receiver = released.lock().unwrap().take().unwrap();
                                // False physically rejects the real connection.
                                if receiver.await.unwrap_or(false) {
                                    respond(&mut socket,"text/plain","peer-wire!").await;
                                }
                            }
                            _ => panic!("unexpected actual HTTP request: {path}"),
                        }
                    });
                }
                completed = tasks.join_next(), if !tasks.is_empty() => { completed.unwrap().unwrap(); }
            }
        }
    });
    Server {
        base,
        held,
        release: Some(release),
        paths,
        task,
    }
}

async fn open(server: &Server) -> PageSession {
    PageSession::open_with_document_response_and_options(
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
    .unwrap()
}
async fn peer(page: &mut PageSession, base: &str) -> (u64, u64) {
    let root = page.auxiliary_root_id().unwrap();
    page.eval_expression("window.open('about:blank','independent-module-peer');void 0")
        .await
        .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.source_page.as_deref() == Some(&root.to_string()))
        .unwrap()
        .target
        .parse()
        .unwrap();
    page.navigate_target(peer, &format!("{base}peer"), "push")
        .await
        .unwrap();
    (root, peer)
}
async fn actual_held_request(page: &mut PageSession, server: &mut Server, root: u64, peer: u64) {
    let held = &mut server.held;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            tokio::select! {
                seen = &mut *held => { seen.unwrap(); break; }
                turn = page.poll_background_work() => { turn.unwrap(); }
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    if outcome.is_err() {
        let paths = server.paths.lock().unwrap().clone();
        let root_network = page
            .network_event_log()
            .target_network_state(&root.to_string());
        let peer_network = page
            .network_event_log()
            .target_network_state(&peer.to_string());
        let peer_state=page.read_target_engine(peer,r#"return JSON.stringify({
          moduleStarted:typeof moduleStarted==='undefined'?null:moduleStarted,
          moduleFinished:typeof moduleFinished==='undefined'?null:moduleFinished,
          outcome:typeof peerModuleOutcome,
          modulePromise:typeof peerModule,
          error:typeof peerModuleError==='undefined'?null:{name:peerModuleError.name,message:peerModuleError.message}
        })"#);
        eprintln!("MODULE_PRECONDITION_PHASE source={} base={} root={} peer={} paths={:?} root_network={} peer_network={} peer_readonly={:?}",file!(),server.base,root,peer,paths,root_network,peer_network,peer_state);
    }
    outcome.expect("peer must start real native HTTP before root evaluation");
}
fn identities(page: &mut PageSession, root: u64, peer: u64, base: &str) {
    let log = page.auxiliary_event_log();
    assert_eq!(log.root(), Some(root.to_string()));
    assert_eq!(log.pages().iter().filter(|info| !info.closed).count(), 2);
    for target in [root, peer] {
        assert!(log
            .pages()
            .iter()
            .any(|info| info.target == target.to_string() && !info.closed));
    }
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("root-document"));
    assert!(page
        .serialize_target(peer)
        .unwrap()
        .contains("peer-document"));
    assert_eq!(page.navigation_snapshot().1, base);
}
fn actual_paths(server: &Server, expected: &[&str]) {
    let mut paths = server.paths.lock().unwrap().clone();
    paths.sort();
    let mut expected: Vec<_> = expected.iter().map(|value| value.to_string()).collect();
    expected.sort();
    assert_eq!(paths, expected);
}

async fn held_module_guard(reject: bool) {
    let mut server = serve().await;
    let mut page = open(&server).await;
    let (root, peer) = peer(&mut page, &server.base).await;
    // This is a genuine registered dynamic module, not a queued fake descriptor.
    page.eval_target_expression(peer,r#"
      globalThis.moduleStarted=false;globalThis.moduleFinished=false;
      globalThis.peerModule=import('/peer-module.mjs');
      globalThis.peerModuleOutcome=peerModule.then(module=>module.receipt,error=>{
        globalThis.peerModuleError=error;return {rejected:true,name:error.name,own:error instanceof TypeError};
      });void 0
    "#,false,false).await.unwrap();
    actual_held_request(&mut page, &mut server, root, peer).await;
    assert_eq!(
        serde_json::from_str::<String>(
            &page
                .read_target_engine(peer, "return [moduleStarted,moduleFinished].join(',')")
                .unwrap()
        )
        .unwrap(),
        "true,false"
    );
    assert_eq!(
        page.network_event_log()
            .target_network_state(&peer.to_string())["pendingRequests"],
        1
    );
    identities(&mut page, root, peer, &server.base);
    actual_paths(&server, &["/peer", "/peer-module.mjs", "/held"]);
    let start = std::time::Instant::now();
    assert_eq!(
        page.eval("globalThis.__RESULT='root-ready'").await.unwrap(),
        "root-ready"
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(
        page.read_target_engine(peer, "return moduleFinished")
            .unwrap(),
        "false"
    );
    assert_eq!(
        page.network_event_log()
            .target_network_state(&peer.to_string())["pendingRequests"],
        1
    );
    identities(&mut page, root, peer, &server.base);
    server.release.take().unwrap().send(!reject).unwrap();
    let result: Value = serde_json::from_str(
        &page
            .eval_target_expression(
                peer,
                "peerModuleOutcome.then(value=>JSON.stringify(value))",
                false,
                false,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    if reject {
        assert_eq!(
            result,
            json!({"rejected":true,"name":"TypeError","own":true})
        );
        assert_eq!(
            page.read_target_engine(peer, "return moduleFinished")
                .unwrap(),
            "false"
        );
        assert_eq!(
            page.eval("globalThis.__RESULT='root-after-peer-rejection'")
                .await
                .unwrap(),
            "root-after-peer-rejection"
        );
    } else {
        assert_eq!(
            result,
            json!({"value":"peer-wire!","url":format!("{}peer-module.mjs",server.base),"document":format!("{}peer",server.base)})
        );
        assert_eq!(
            page.read_target_engine(peer, "return moduleFinished")
                .unwrap(),
            "true"
        );
    }
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_root_scalar_does_not_wait_for_peer_registered_module_tla() {
    held_module_guard(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn handled_peer_module_wire_rejection_preserves_its_realm_and_root_progress() {
    held_module_guard(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_root_void_module_reaches_own_http_while_peer_fetch_is_held() {
    let mut server = serve().await;
    let mut page = open(&server).await;
    let (root, peer) = peer(&mut page, &server.base).await;
    page.eval_target_expression(peer,"globalThis.peerSettled=false;globalThis.peerFetch=fetch('/held').then(()=>{peerSettled=true},()=>{peerSettled=true});void 0",false,false).await.unwrap();
    actual_held_request(&mut page, &mut server, root, peer).await;
    identities(&mut page, root, peer, &server.base);
    let start = std::time::Instant::now();
    let value = page
        .eval(
            r#"
      globalThis.__RESULT='module-not-finished';
      const element=document.createElement('script');element.type='module';
      element.src='/root-module.mjs';document.body.append(element);void 0;
    "#,
        )
        .await
        .unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    let value: Value = serde_json::from_str(&value).unwrap();
    assert_eq!(
        value,
        json!({"value":"root-wire!","url":format!("{}root-module.mjs",server.base),"document":server.base})
    );
    assert_eq!(
        page.network_event_log()
            .target_network_state(&peer.to_string())["pendingRequests"],
        1
    );
    assert_eq!(
        page.read_target_engine(peer, "return peerSettled").unwrap(),
        "false"
    );
    identities(&mut page, root, peer, &server.base);
    actual_paths(&server, &["/peer", "/held", "/root-module.mjs", "/own"]);
    page.close();
}
