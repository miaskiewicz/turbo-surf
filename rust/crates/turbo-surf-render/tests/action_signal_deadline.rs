//! Signal completion releases failed navigations, while actual action deadlines reject.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};
use turbo_surf_render::PageSession;

struct Fixture {
    origin: String,
    release: mpsc::Sender<()>,
    requests: mpsc::Receiver<String>,
    held_headers: Arc<tokio::sync::Notify>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Fixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (release, released) = mpsc::channel();
        let (send, requests) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let held_headers = Arc::new(tokio::sync::Notify::new());
        let headers_sent = held_headers.clone();
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !stopping.load(Ordering::SeqCst) && Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let request = read_request(&mut stream);
                send.send(request.clone()).unwrap();
                if request.contains(" /reject ") {
                    drop(stream);
                } else if request.contains(" /held ") {
                    let body = "<!doctype html><p>replacement</p>";
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n<!doctype html>",body.len()).unwrap();
                    stream.flush().unwrap();
                    headers_sent.notify_one();
                    let _ = released.recv_timeout(Duration::from_secs(10));
                    let _ = stream.write_all(b"<p>replacement</p>");
                } else {
                    let body = "<!doctype html><p id='recovered'>recovered</p>";
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                }
            }
        });
        Self {
            origin,
            release,
            requests,
            held_headers,
            stop,
            thread: Some(thread),
        }
    }
    fn request(&self) -> String {
        self.requests.recv_timeout(Duration::from_secs(2)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.release.send(());
        self.thread.take().unwrap().join().unwrap();
    }
}
fn read_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let mut bytes = [0; 1024];
        let size = stream.read(&mut bytes).unwrap();
        assert_ne!(size, 0, "client closed before sending request headers");
        request.extend_from_slice(&bytes[..size]);
        assert!(request.len() <= 8192, "fixture request exceeds its bound");
    }
    String::from_utf8(request)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .into()
}

async fn assert_signal_deadline(seed_peer_failure: bool, stall_executor: bool) {
    let fixture = Fixture::start();
    // This existing public budget governs native driver eval, fetch and commit wait.
    let mut page = PageSession::open(
        "<p id='old'>old document</p>",
        &fixture.origin,
        "",
        "",
        1000,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let network = page.network_event_log();
    let baseline = network.target_network_state(&root.to_string())["navigationRevision"]
        .as_str()
        .unwrap()
        .to_owned();
    let peer_revision = if seed_peer_failure {
        page.eval("window.open('about:blank','peer')")
            .await
            .unwrap();
        let peer = page
            .auxiliary_event_log()
            .pages()
            .into_iter()
            .find(|entry| entry.target != root.to_string())
            .unwrap()
            .target
            .parse()
            .unwrap();
        assert!(page
            .dispatch_target_engine(peer, "location.assign('/reject');")
            .await
            .is_err());
        assert_eq!(fixture.request(), "GET /reject HTTP/1.1");
        Some(network.target_network_state(&peer.to_string())["navigationRevision"].clone())
    } else {
        None
    };
    page.eval_target_engine(root, "location.assign('/held');", false)
        .await
        .unwrap();
    if let Some(peer_revision) = peer_revision {
        assert_eq!(
            network.target_network_state(&root.to_string())["navigationRevision"],
            peer_revision,
            "the peer failure must have the same admission serial to exercise target ownership"
        );
    }
    let blocker = stall_executor.then(|| {
        let headers = fixture.held_headers.clone();
        let block_at = tokio::time::Instant::now() + Duration::from_millis(900);
        tokio::spawn(async move {
            headers.notified().await;
            tokio::time::sleep_until(block_at).await;
            // Reproduce a late executor poll while the genuine native deadline expires.
            std::thread::sleep(Duration::from_millis(200));
        })
    });
    let result = page
        .dispatch_target_engine_since(root, "", Some(&baseline))
        .await;
    if let Some(blocker) = blocker {
        blocker.await.unwrap();
    }
    assert!(result.is_err(), "held own HTML must reject at the actual native budget, never release as navigation-failed: {result:?}");
    assert_eq!(
        fixture.request(),
        "GET /held HTTP/1.1",
        "the held navigation must reach real HTTP"
    );
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("old document"));
    fixture.release.send(()).unwrap();
    page.dispatch_target_engine(root, "location.assign('/recovered');")
        .await
        .unwrap();
    assert_eq!(fixture.request(), "GET /recovered HTTP/1.1");
    assert!(
        page.serialize_target(root)
            .unwrap()
            .contains("id=\"recovered\""),
        "dropping the prior waiter must not cancel a later generation"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn signal_only_completion_preserves_the_actual_native_action_deadline() {
    assert_signal_deadline(false, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn matching_peer_failure_cannot_release_an_owned_navigation_deadline() {
    assert_signal_deadline(true, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_navigation_signal_release_keeps_later_generation_live() {
    let fixture = Fixture::start();
    let mut page = PageSession::open("<p>old document</p>", &fixture.origin, "", "", 1000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let baseline = page
        .network_event_log()
        .target_network_state(&root.to_string())["navigationRevision"]
        .as_str()
        .unwrap()
        .to_owned();
    page.eval_target_engine(root, "location.assign('/reject');", false)
        .await
        .unwrap();
    let outcome = page
        .dispatch_target_engine_since(root, "", Some(&baseline))
        .await
        .unwrap();
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();
    assert_eq!(outcome["status"], "navigation-failed");
    assert_eq!(fixture.request(), "GET /reject HTTP/1.1");
    page.dispatch_target_engine(root, "location.assign('/recovered');")
        .await
        .unwrap();
    assert_eq!(fixture.request(), "GET /recovered HTTP/1.1");
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("id=\"recovered\""));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn late_signal_timeout_cleans_up_its_owned_navigation_before_recovery() {
    assert_signal_deadline(true, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_an_expired_signal_completion_preserves_healthy_owner_entry() {
    let fixture = Fixture::start();
    let mut page = PageSession::open("<p>old document</p>", &fixture.origin, "", "", 1000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let network = page.network_event_log();
    let baseline = network.target_network_state(&root.to_string())["navigationRevision"]
        .as_str()
        .unwrap()
        .to_owned();
    page.eval_target_engine(root, "location.assign('/held');", false)
        .await
        .unwrap();
    let mut completion = Box::pin(page.dispatch_target_engine_since(root, "", Some(&baseline)));
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            biased;
            result = &mut completion => panic!("held HTTP body must remain pending: {result:?}"),
            _ = fixture.held_headers.notified() => {},
        }
    })
    .await
    .expect("the owned navigation must reach real held HTTP");
    assert_eq!(fixture.request(), "GET /held HTTP/1.1");
    // The runtime cannot poll a canceled caller while its native watchdog expires.
    std::thread::sleep(Duration::from_millis(1100));
    drop(completion);
    fixture.release.send(()).unwrap();
    page.dispatch_target_engine(root, "location.assign('/recovered');")
        .await
        .unwrap();
    assert_eq!(fixture.request(), "GET /recovered HTTP/1.1");
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("id=\"recovered\""));
    page.close();
}
