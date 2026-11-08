//! The native WebSocket client (#144) against localhost servers: Chrome 156's handshake
//! head, response validation, echo over ws: and wss: (a test trust anchor), server ping
//! and close handling, and abnormal closure. The full browser contract is covered by
//! turbo-surf-render's tests/network_transports.rs against the Chrome 156 control.
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::websocket::{
    accept_key, handshake_head, open, validate_response, WsCommand, WsEvent, WsRequest,
};

const CERT: &[u8] = include_bytes!("../../turbo-surf-render/tests/csp_report_cert.der");
const KEY: &[u8] = include_bytes!("../../turbo-surf-render/tests/csp_report_key.der");

#[test]
fn handshake_head_follows_chrome_order() {
    let request = WsRequest {
        url: "ws://127.0.0.1:9/chat?room=1".into(),
        origin: "http://127.0.0.1:9".into(),
        protocols: vec!["a".into(), "b".into()],
        user_agent: "UA".into(),
        accept_language: "en-US,en;q=0.9".into(),
        cookie: "c=1".into(),
        trust_anchor: None,
    };
    let url = url::Url::parse(&request.url).unwrap();
    let head = handshake_head(&request, &url, "KEY");
    let names: Vec<&str> = head
        .split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':').map(|(name, _)| name))
        .collect();
    assert!(head.starts_with("GET /chat?room=1 HTTP/1.1\r\nHost: 127.0.0.1:9\r\n"));
    assert_eq!(
        names,
        [
            "Host",
            "Connection",
            "Pragma",
            "Cache-Control",
            "User-Agent",
            "Upgrade",
            "Origin",
            "Sec-WebSocket-Version",
            "Accept-Encoding",
            "Accept-Language",
            "Cookie",
            "Sec-WebSocket-Key",
            "Sec-WebSocket-Extensions",
            "Sec-WebSocket-Protocol"
        ]
    );
    assert!(head.ends_with("Sec-WebSocket-Protocol: a, b\r\n\r\n"));
    let bare = WsRequest {
        url: "wss://example.test/".into(),
        ..WsRequest::default()
    };
    let head = handshake_head(&bare, &url::Url::parse(&bare.url).unwrap(), "K");
    assert!(head.contains("Host: example.test\r\n"));
    assert!(!head.contains("Cookie") && !head.contains("Accept-Language"));
    assert!(!head.contains("Sec-WebSocket-Protocol"));
}

#[test]
fn response_validation_fails_like_chrome() {
    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    assert_eq!(accept_key(key), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    let ok = |extra: &str| {
        format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: WebSocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Accept: {}\r\n{extra}",
            accept_key(key)
        )
    };
    let offered = ["chat".to_owned()];
    let accepted = validate_response(
        &ok("Sec-WebSocket-Protocol: chat\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nSec-WebSocket-Extensions: permessage-deflate\r\n"),
        key,
        &offered,
    )
    .unwrap();
    assert_eq!(accepted.protocol, "chat");
    assert_eq!(accepted.extensions, "permessage-deflate");
    assert_eq!(accepted.set_cookies, ["a=1", "b=2"]);
    for bad in [
        "HTTP/1.1 200 OK\r\n".to_owned(),
        "HTTP/1.1 101 Switching\r\nConnection: Upgrade\r\n".to_owned(),
        format!("HTTP/1.1 101 x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\nSec-WebSocket-Accept: {0}\r\n", accept_key(key)),
        ok("Sec-WebSocket-Protocol: other\r\n"),
        ok("Sec-WebSocket-Extensions: x-unknown\r\n"),
    ] {
        assert!(validate_response(&bad, key, &offered).is_err(), "{bad}");
    }
    assert!(validate_response(&ok("Sec-WebSocket-Protocol: chat\r\n"), key, &[]).is_err());
}

/// Accepts one connection, completes the handshake and runs `script` on the stream.
async fn serve<S, F, Fut>(stream: S, script: F)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    F: FnOnce(S) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut stream = stream;
    let mut head = Vec::new();
    let mut byte = [0u8];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }
    let text = String::from_utf8(head).unwrap();
    let key = text
        .lines()
        .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
        .unwrap();
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\nSet-Cookie: s=1\r\n\r\n",
        accept_key(key)
    );
    stream.write_all(response.as_bytes()).await.unwrap();
    script(stream).await;
}

async fn read_client_frame<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> (u8, Vec<u8>) {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await.unwrap();
    assert_eq!(head[1] & 0x80, 0x80, "client frames are masked");
    let mut mask = [0u8; 4];
    stream.read_exact(&mut mask).await.unwrap();
    let mut payload = vec![0u8; usize::from(head[1] & 0x7f)];
    stream.read_exact(&mut payload).await.unwrap();
    for (at, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[at & 3];
    }
    (head[0] & 0x0f, payload)
}

/// Echo one text message, ping, then close with 4100 and wait for the client's echo.
async fn echo_then_close<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(mut stream: S) {
    let (opcode, payload) = read_client_frame(&mut stream).await;
    assert_eq!(opcode, 1);
    let mut reply = vec![0x81, payload.len() as u8];
    reply.extend_from_slice(&payload);
    reply.extend_from_slice(&[0x89, 1, b'p']);
    stream.write_all(&reply).await.unwrap();
    assert_eq!(read_client_frame(&mut stream).await, (10, b"p".to_vec()));
    stream
        .write_all(&[0x88, 4, 0x10, 0x04, b'o', b'k'])
        .await
        .unwrap();
    assert_eq!(read_client_frame(&mut stream).await, (8, vec![0x10, 0x04]));
    stream.shutdown().await.unwrap();
}

async fn expect_session(request: WsRequest) {
    let (handle, mut events) = open(request);
    match events.recv().await.unwrap() {
        WsEvent::Open(accepted) => assert_eq!(accepted.set_cookies, ["s=1"]),
        other => panic!("{other:?}"),
    }
    handle.send(WsCommand::Text("hi".into()));
    assert_eq!(events.recv().await, Some(WsEvent::Sent(2)));
    assert_eq!(events.recv().await, Some(WsEvent::Text("hi".into())));
    assert_eq!(
        events.recv().await,
        Some(WsEvent::Closed {
            clean: true,
            code: 4100,
            reason: "ok".into()
        })
    );
}

#[tokio::test]
async fn echoes_pings_and_closes_over_ws() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        serve(socket, echo_then_close).await;
    });
    expect_session(WsRequest {
        url: format!("ws://127.0.0.1:{port}/"),
        ..WsRequest::default()
    })
    .await;
    server.await.unwrap();
}

fn tls_acceptor() -> tokio_rustls::TlsAcceptor {
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

#[tokio::test]
async fn echoes_over_wss_with_a_trust_anchor() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tls_acceptor();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let tls = acceptor.accept(socket).await.unwrap();
        serve(tls, echo_then_close).await;
    });
    expect_session(WsRequest {
        url: format!("wss://127.0.0.1:{port}/"),
        trust_anchor: Some(CERT.to_vec()),
        ..WsRequest::default()
    })
    .await;
    server.await.unwrap();
}

// wss: rides the https transport (#179); the head on the wire must still be Chrome's,
// byte for byte, with nothing the HTTP client would add. A frame sent in the same write
// as the 101 head reaches the session.
#[tokio::test]
async fn wss_sends_chromes_head_byte_for_byte() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tls_acceptor();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut tls = acceptor.accept(socket).await.unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8];
        while !head.ends_with(b"\r\n\r\n") {
            tls.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        let key = head
            .lines()
            .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
            .unwrap()
            .to_owned();
        let mut reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\nSec-WebSocket-Protocol: chat\r\n\r\n",
            accept_key(&key)
        )
        .into_bytes();
        reply.extend_from_slice(&[0x81, 5]);
        reply.extend_from_slice(b"early");
        tls.write_all(&reply).await.unwrap();
        let mut rest = Vec::new();
        let _ = tls.read_to_end(&mut rest).await;
        (head, key)
    });
    let request = WsRequest {
        url: format!("wss://127.0.0.1:{port}/chat?room=1"),
        origin: format!("https://127.0.0.1:{port}"),
        protocols: vec!["chat".into()],
        user_agent: "UA".into(),
        accept_language: "en-US,en;q=0.9".into(),
        cookie: "c=1".into(),
        trust_anchor: Some(CERT.to_vec()),
    };
    let (handle, mut events) = open(request.clone());
    match events.recv().await.unwrap() {
        WsEvent::Open(accepted) => assert_eq!(accepted.protocol, "chat"),
        other => panic!("{other:?}"),
    }
    assert_eq!(events.recv().await, Some(WsEvent::Text("early".into())));
    drop(handle);
    let (head, key) = server.await.unwrap();
    let url = url::Url::parse(&request.url).unwrap();
    assert_eq!(head, handshake_head(&request, &url, &key));
}

#[tokio::test]
async fn failures_and_abnormal_closure() {
    // Refused connection and an untrusted certificate fail the connection.
    let refused = WsRequest {
        url: "ws://127.0.0.1:1/".into(),
        ..WsRequest::default()
    };
    assert_eq!(open(refused).1.recv().await, Some(WsEvent::Failed));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        drop(socket);
        let (socket, _) = listener.accept().await.unwrap();
        serve(socket, |mut stream| async move {
            // A masked server frame is a protocol error.
            stream
                .write_all(&[0x81, 0x81, 0, 0, 0, 0, b'x'])
                .await
                .unwrap();
            let mut rest = Vec::new();
            let _ = stream.read_to_end(&mut rest).await;
        })
        .await;
        let (socket, _) = listener.accept().await.unwrap();
        serve(socket, |stream| async move { drop(stream) }).await;
    });
    let untrusted = WsRequest {
        url: format!("wss://127.0.0.1:{port}/"),
        ..WsRequest::default()
    };
    assert_eq!(open(untrusted).1.recv().await, Some(WsEvent::Failed));
    let plain = WsRequest {
        url: format!("ws://127.0.0.1:{port}/"),
        ..WsRequest::default()
    };
    let (_handle, mut events) = open(plain.clone());
    assert!(matches!(events.recv().await, Some(WsEvent::Open(_))));
    assert_eq!(events.recv().await, Some(WsEvent::Failed));
    let (_handle, mut events) = open(plain);
    assert!(matches!(events.recv().await, Some(WsEvent::Open(_))));
    assert_eq!(
        events.recv().await,
        Some(WsEvent::Closed {
            clean: false,
            code: 1006,
            reason: String::new()
        })
    );
    server.await.unwrap();
}

// The closing handshake must not wait forever when the server never answers our close
// frame. Chrome 156 forces the connection closed after ~5s with wasClean=false and code
// 1006 (turbo-surf#178, found while fixing #144).
#[tokio::test(start_paused = true)]
async fn close_handshake_times_out_when_the_server_goes_silent() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        serve(socket, |mut stream| async move {
            // Read the client's close frame but never answer; hold the socket open.
            let (opcode, _) = read_client_frame(&mut stream).await;
            assert_eq!(opcode, 8);
            let mut rest = Vec::new();
            let _ = stream.read_to_end(&mut rest).await;
        })
        .await;
    });
    let (handle, mut events) = open(WsRequest {
        url: format!("ws://127.0.0.1:{port}/"),
        ..WsRequest::default()
    });
    assert!(matches!(events.recv().await, Some(WsEvent::Open(_))));
    handle.send(WsCommand::Close(Some(1000), "bye".into()));
    assert_eq!(
        events.recv().await,
        Some(WsEvent::Closed {
            clean: false,
            code: 1006,
            reason: String::new(),
        })
    );
    drop(handle);
    let _ = server.await;
}

// A wss: handshake answered with anything but 101 fails the connection; a redirect is
// not followed (Chrome fails it too).
#[tokio::test]
async fn wss_non_switching_responses_fail() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tls_acceptor();
    let replies = [
        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_owned(),
        format!("HTTP/1.1 302 Found\r\nLocation: wss://127.0.0.1:{port}/next\r\nContent-Length: 0\r\n\r\n"),
    ];
    let server = tokio::spawn(async move {
        let mut accepted = 0;
        for reply in replies {
            let (socket, _) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(socket).await.unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8];
            while !head.ends_with(b"\r\n\r\n") {
                tls.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
            }
            tls.write_all(reply.as_bytes()).await.unwrap();
            tls.shutdown().await.unwrap();
            accepted += 1;
        }
        accepted
    });
    for _ in 0..2 {
        let request = WsRequest {
            url: format!("wss://127.0.0.1:{port}/"),
            trust_anchor: Some(CERT.to_vec()),
            ..WsRequest::default()
        };
        assert_eq!(open(request).1.recv().await, Some(WsEvent::Failed));
    }
    assert_eq!(server.await.unwrap(), 2);
}
