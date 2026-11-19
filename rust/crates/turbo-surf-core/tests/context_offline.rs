use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use turbo_surf_core::http_cache::HttpCache;
use turbo_surf_core::net::{fetch_html, FetchOptions};
use turbo_surf_core::request_context::ClientHints;

struct Server {
    origin: String,
    paths: Arc<Mutex<Vec<String>>>,
    held: Arc<Mutex<BTreeMap<String, oneshot::Sender<()>>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let paths = Arc::new(Mutex::new(Vec::new()));
        let held = Arc::new(Mutex::new(BTreeMap::new()));
        let log = paths.clone();
        let releases = held.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let log = log.clone();
                let releases = releases.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        if socket.read_exact(&mut byte).await.is_err() {
                            return;
                        }
                        head.push(byte[0]);
                    }
                    let request = String::from_utf8(head).unwrap();
                    let path = request.split_whitespace().nth(1).unwrap().to_owned();
                    log.lock().unwrap().push(path.clone());
                    if matches!(path.as_str(), "/held" | "/redirect") {
                        let (send, receive) = oneshot::channel();
                        releases.lock().unwrap().insert(path.clone(), send);
                        let _ = receive.await;
                    }
                    let response = if path == "/redirect" {
                        "HTTP/1.1 302 Found\r\nLocation: /redirect-target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    } else {
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nCache-Control: max-age=600\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                    };
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            origin,
            paths,
            held,
            task,
        }
    }
    async fn release(&self, path: &str) {
        for _ in 0..100 {
            if let Some(send) = self.held.lock().unwrap().remove(path) {
                send.send(()).unwrap();
                return;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("held localhost request did not start");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_offline_blocks_new_sends_but_keeps_cache_and_independent_contexts() {
    let server = Server::new().await;
    let hints = ClientHints::default();
    let tab = hints.clone();
    let api = hints.with_independent_network_conditions();
    let other = ClientHints::default();
    let cache = HttpCache::default();
    let cached = format!("{}/cached", server.origin);
    assert_eq!(
        fetch_html(
            &cached,
            FetchOptions {
                client_hints: Some(&hints),
                http_cache: Some(&cache),
                ..Default::default()
            }
        )
        .await
        .unwrap()
        .html,
        "ok"
    );
    hints.set_offline(true);
    assert!(tab.is_offline());
    assert!(!api.is_offline());
    assert!(!other.is_offline());
    assert_eq!(
        fetch_html(
            &cached,
            FetchOptions {
                client_hints: Some(&tab),
                http_cache: Some(&cache),
                ..Default::default()
            }
        )
        .await
        .unwrap()
        .html,
        "ok"
    );
    for method in ["GET", "POST"] {
        let outcome = fetch_html(
            &format!("{}/blocked-{method}", server.origin),
            FetchOptions {
                client_hints: Some(&tab),
                method: Some(method.into()),
                body: (method == "POST").then(|| "companyB".into()),
                ..Default::default()
            },
        )
        .await;
        assert!(
            outcome.is_err(),
            "offline {method} must not reach transport"
        );
    }
    let reporter = turbo_surf_core::net::build_report_client()
        .with_network_conditions(hints.network_conditions());
    for modern in [false, true] {
        assert!(turbo_surf_core::net::send_report_request(
            &reporter,
            "POST",
            &format!("{}/report", server.origin),
            &BTreeMap::new(),
            Vec::new(),
            modern
        )
        .await
        .is_err());
    }
    for context in [&api, &other] {
        assert!(fetch_html(
            &format!("{}/independent", server.origin),
            FetchOptions {
                client_hints: Some(context),
                ..Default::default()
            }
        )
        .await
        .is_ok());
    }
    hints.set_offline(false);
    assert!(fetch_html(
        &format!("{}/restored", server.origin),
        FetchOptions {
            client_hints: Some(&tab),
            ..Default::default()
        }
    )
    .await
    .is_ok());
    let paths = server.paths.lock().unwrap();
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.as_str() == "/cached")
            .count(),
        1
    );
    assert!(!paths
        .iter()
        .any(|path| path.contains("blocked") || path == "/report"));
}

#[tokio::test(flavor = "current_thread")]
async fn offline_does_not_cancel_started_http_but_rejects_the_redirect_hop() {
    let server = Server::new().await;
    let hints = ClientHints::default();
    let url = format!("{}/held", server.origin);
    let child = hints.clone();
    let request = tokio::spawn(async move {
        fetch_html(
            &url,
            FetchOptions {
                client_hints: Some(&child),
                headers: BTreeMap::from([("x-entity-id".into(), "companyB".into())]),
                ..Default::default()
            },
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !server.held.lock().unwrap().contains_key("/held") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("held request starts");
    hints.set_offline(true);
    server.release("/held").await;
    assert_eq!(request.await.unwrap().unwrap().html, "ok");
    hints.set_offline(false);
    let url = format!("{}/redirect", server.origin);
    let child = hints.clone();
    let request = tokio::spawn(async move {
        fetch_html(
            &url,
            FetchOptions {
                client_hints: Some(&child),
                ..Default::default()
            },
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !server.held.lock().unwrap().contains_key("/redirect") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("redirect request starts");
    hints.set_offline(true);
    server.release("/redirect").await;
    assert!(request.await.unwrap().is_err());
    assert!(!server
        .paths
        .lock()
        .unwrap()
        .iter()
        .any(|path| path == "/redirect-target"));
}
