//! A session's trust anchor covers its `wss:` sockets as well as its https fetches
//! (#188): one localhost TLS server signed by the test certificate answers both, and
//! in one render session both succeed with the anchor configured and both fail without.
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::rustls;
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

const CERT: &[u8] = include_bytes!("csp_report_cert.der");
const KEY: &[u8] = include_bytes!("csp_report_key.der");

fn acceptor() -> tokio_rustls::TlsAcceptor {
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
}

/// Answers a WebSocket upgrade with 101, one "hi" text frame and a 1000 close;
/// anything else with a 200 "fetched" body.
async fn connection(socket: tokio::net::TcpStream, acceptor: tokio_rustls::TlsAcceptor) {
    let Ok(mut tls) = acceptor.accept(socket).await else {
        return; // the untrusted client aborts the handshake
    };
    let mut head = Vec::new();
    let mut byte = [0u8];
    while !head.ends_with(b"\r\n\r\n") {
        if tls.read_exact(&mut byte).await.is_err() {
            return;
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    // Header names are matched without case: the non-impersonate backend lowercases them.
    let key = head.split("\r\n").find_map(|line| {
        let (name, value) = line.split_once(": ")?;
        name.eq_ignore_ascii_case("sec-websocket-key")
            .then_some(value)
    });
    let response = match key {
        Some(key) => format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            turbo_surf_core::websocket::accept_key(key)
        )
        .into_bytes()
        .into_iter()
        .chain([0x81, 2, b'h', b'i', 0x88, 2, 0x03, 0xe8])
        .collect::<Vec<u8>>(),
        None => b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 7\r\nConnection: close\r\n\r\nfetched".to_vec(),
    };
    let _ = tls.write_all(&response).await;
    let _ = tls.flush().await;
    // Let the client's close frame (or EOF) arrive before dropping the connection.
    let mut rest = [0u8; 64];
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), tls.read(&mut rest)).await;
    let _ = tls.shutdown().await;
}

/// The localhost TLS server on its own thread and runtime; returns its port.
fn start_server() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let acceptor = acceptor();
                while let Ok((socket, _)) = listener.accept().await {
                    tokio::spawn(connection(socket, acceptor.clone()));
                }
            });
    });
    port
}

async fn observe(hints: ClientHints) -> String {
    let port = start_server();
    let base = format!("https://127.0.0.1:{port}/");
    let mut page = PageSession::open_with_request_state(
        "<!doctype html><body>",
        &base,
        "",
        "",
        20000,
        "",
        hints,
    )
    .await
    .unwrap();
    let probe = format!(
        r#"(async()=>{{
      let fetched;
      try{{fetched=await (await fetch('/data')).text();}}catch(e){{fetched='error:'+e.name;}}
      const socket=await new Promise(resolve=>{{
        const ws=new WebSocket('wss://127.0.0.1:{port}/ws');
        ws.onmessage=e=>resolve('message:'+e.data);
        ws.onerror=()=>resolve('error');
      }});
      return fetched+'|'+socket;
    }})()"#
    );
    let result = page.eval_expression(&probe).await.unwrap();
    page.close();
    result
}

#[tokio::test(flavor = "current_thread")]
async fn fetch_and_wss_share_the_sessions_trust_anchor() {
    assert_eq!(
        observe(ClientHints::with_trust_anchor(CERT.to_vec())).await,
        "fetched|message:hi"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn fetch_and_wss_both_fail_without_the_anchor() {
    assert_eq!(
        observe(ClientHints::default()).await,
        "error:TypeError|error"
    );
}
