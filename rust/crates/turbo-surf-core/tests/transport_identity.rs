//! turbo-surf#113: HTTP/1.1 Connection/Priority, Sec-CH-DPR and the Critical-CH
//! restart, compared with the Chrome 156 loopback control
//! (`fixtures/transport_identity_chrome156.json`, recorded by
//! harness/detection/transport-identity-control.mjs). Raw HTTP/1.1 wire only:
//! the servers keep each header line's spelling and order.
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use turbo_surf_core::{
    net::{self, FetchOptions},
    request_context::{ClientHints, RequestContext},
};

const CERT: &[u8] = include_bytes!("../../turbo-surf-render/tests/csp_report_cert.der");
const KEY: &[u8] = include_bytes!("../../turbo-surf-render/tests/csp_report_key.der");
const CONTROL: &str = include_str!("fixtures/transport_identity_chrome156.json");

type Lines = Vec<(String, String)>;
type Log = Arc<Mutex<Vec<(String, Lines)>>>;

fn control() -> Value {
    serde_json::from_str(CONTROL).unwrap()
}

fn chrome(kind: &str) -> Vec<Lines> {
    control()["requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|request| request["kind"] == kind)
        .map(|request| {
            request["headers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    (
                        pair[0].as_str().unwrap().to_owned(),
                        pair[1].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        })
        .collect()
}

fn get<'a>(lines: &'a Lines, name: &str) -> Option<&'a str> {
    lines
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

// Accept-CH/Critical-CH exactly as the battery's `critical` route sends them.
fn response_for(path: &str) -> String {
    let mut head = String::from(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nCache-Control: no-store\r\nContent-Length: 2\r\n",
    );
    if path.contains("critical") {
        head.push_str("Accept-CH: Sec-CH-UA-Full-Version-List, Sec-CH-UA-Arch\r\n");
        head.push_str("Critical-CH: Sec-CH-UA-Full-Version-List\r\n");
    }
    if path.contains("unsatisfiable") {
        head.push_str("Critical-CH: Sec-CH-UA-Model\r\n");
    }
    if path.contains("dpr") {
        head.push_str("Accept-CH: Sec-CH-DPR\r\n");
    }
    head.push_str("\r\nok");
    head
}

async fn serve<S: AsyncRead + AsyncWrite + Unpin>(mut socket: S, log: Log) {
    loop {
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            if socket.read(&mut byte).await.unwrap_or(0) == 0 {
                return;
            }
            request.push(byte[0]);
        }
        let text = String::from_utf8(request).unwrap();
        let path = text.split_whitespace().nth(1).unwrap().to_owned();
        let lines = text
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(key, value)| (key.to_owned(), value.trim().to_owned()))
            .collect();
        log.lock().unwrap().push((path.clone(), lines));
        if socket
            .write_all(response_for(&path).as_bytes())
            .await
            .is_err()
        {
            return;
        }
    }
}

struct Server {
    origin: String,
    log: Log,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    /// `tls` serves HTTPS without ALPN, so the client settles on HTTP/1.1.
    async fn start(tls: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let scheme = if tls { "https" } else { "http" };
        let origin = format!("{scheme}://{}", listener.local_addr().unwrap());
        let log: Log = Arc::default();
        let output = log.clone();
        let acceptor = tls.then(|| {
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(CERT.to_vec())],
                rustls::pki_types::PrivatePkcs8KeyDer::from(KEY.to_vec()).into(),
            )
            .unwrap();
            tokio_rustls::TlsAcceptor::from(Arc::new(config))
        });
        let task = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let log = output.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor {
                        Some(acceptor) => {
                            if let Ok(socket) = acceptor.accept(socket).await {
                                serve(socket, log).await;
                            }
                        }
                        None => serve(socket, log).await,
                    }
                });
            }
        });
        Self { origin, log, task }
    }
    fn requests(&self, path: &str) -> Vec<Lines> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|(seen, _)| seen == path)
            .map(|(_, lines)| lines.clone())
            .collect()
    }
}

fn hints() -> ClientHints {
    ClientHints::with_client_builder(|| net::client_builder_with_trust(CERT))
}

async fn navigate(url: &str, hints: &ClientHints) {
    net::fetch_html(
        url,
        FetchOptions {
            allow_non_html: true,
            max_redirects: Some(10),
            client_hints: Some(hints),
            context: Some(RequestContext::navigation(None, true)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn http1_navigation_sends_connection_keep_alive_in_chromes_slot() {
    let chrome = &chrome("h1-nav")[0];
    let server = Server::start(true).await;
    navigate(&format!("{}/h1-nav", server.origin), &hints()).await;
    let sent = &server.requests("/h1-nav")[0];
    // Chrome: `Host`, then `Connection: keep-alive`, before every other header.
    assert_eq!(chrome[1].0, "Connection");
    assert_eq!(
        &sent[..2],
        &chrome[..2]
            .iter()
            .map(|(name, value)| {
                let value = if name == "Host" {
                    sent[0].1.clone()
                } else {
                    value.clone()
                };
                (name.clone(), value)
            })
            .collect::<Vec<_>>()[..]
    );
}

#[tokio::test]
async fn cleartext_navigation_matches_chromes_header_set_without_priority() {
    let chrome = &chrome("http-nav")[0];
    assert_eq!(get(chrome, "priority"), None);
    let server = Server::start(false).await;
    navigate(&format!("{}/http-nav", server.origin), &hints()).await;
    let sent = &server.requests("/http-nav")[0];
    assert_eq!(get(sent, "priority"), None, "{sent:?}");
    assert_eq!(get(sent, "connection"), get(chrome, "connection"));
}

#[tokio::test]
async fn an_origin_known_to_speak_http1_gets_no_priority_header() {
    let server = Server::start(true).await;
    let hints = hints();
    navigate(&format!("{}/first", server.origin), &hints).await;
    navigate(&format!("{}/second", server.origin), &hints).await;
    let second = &server.requests("/second")[0];
    assert_eq!(get(second, "priority"), None, "{second:?}");
    assert_eq!(get(second, "connection"), Some("keep-alive"));
}

#[tokio::test]
async fn critical_ch_restarts_once_with_the_accepted_hints_in_chromes_order() {
    let chrome = chrome("critical");
    assert_eq!(chrome.len(), 2, "Chrome restarts the navigation once");
    let server = Server::start(true).await;
    navigate(&format!("{}/critical", server.origin), &hints()).await;
    let sent = server.requests("/critical");
    assert_eq!(sent.len(), 2, "{sent:?}");
    for hint in ["sec-ch-ua-full-version-list", "sec-ch-ua-arch"] {
        assert_eq!(
            get(&sent[0], hint).is_some(),
            get(&chrome[0], hint).is_some()
        );
        assert_eq!(
            get(&sent[1], hint).is_some(),
            get(&chrome[1], hint).is_some()
        );
    }
    assert_eq!(
        get(&sent[1], "sec-ch-ua-full-version-list"),
        get(&chrome[1], "sec-ch-ua-full-version-list")
    );
    // The restart's relative order of the names both sides send.
    let names = |lines: &Lines| -> Vec<String> {
        lines
            .iter()
            .map(|(name, _)| name.to_ascii_lowercase())
            .collect()
    };
    let (ours, theirs) = (names(&sent[1]), names(&chrome[1]));
    let shared = |list: &Vec<String>, other: &Vec<String>| -> Vec<String> {
        list.iter()
            .filter(|name| other.contains(name))
            .cloned()
            .collect()
    };
    assert_eq!(shared(&ours, &theirs), shared(&theirs, &ours));
}

#[tokio::test]
async fn critical_ch_does_not_restart_for_a_hint_that_was_not_accepted() {
    let server = Server::start(true).await;
    navigate(&format!("{}/unsatisfiable", server.origin), &hints()).await;
    assert_eq!(server.requests("/unsatisfiable").len(), 1);
}

#[tokio::test]
async fn sec_ch_dpr_is_offered_after_an_accept_ch_opt_in() {
    let chrome = &chrome("ch-next")[0];
    assert!(get(chrome, "sec-ch-dpr").is_some());
    let server = Server::start(true).await;
    let hints = hints();
    navigate(&format!("{}/dpr", server.origin), &hints).await;
    navigate(&format!("{}/next", server.origin), &hints).await;
    let next = &server.requests("/next")[0];
    // The document's devicePixelRatio: the render tier's default (runtime.rs).
    assert_eq!(get(next, "sec-ch-dpr"), Some("2"), "{next:?}");
}
