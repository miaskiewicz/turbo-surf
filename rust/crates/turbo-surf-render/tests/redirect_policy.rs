//! Redirect credential boundaries and response filtering use the real localhost transport.
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;
type Requests = Arc<Mutex<Vec<(String, bool, String)>>>;

async fn reply(
    mut socket: tokio::net::TcpStream,
    source: String,
    cross: String,
    requests: Requests,
) {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 1024];
        let count = socket.read(&mut chunk).await.unwrap();
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8_lossy(&bytes);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let method = request.split_whitespace().next().unwrap_or("GET");
    let hop = request
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with("x-hop:"));
    let authorization = request
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with("authorization:"));
    requests
        .lock()
        .unwrap()
        .push((path.to_owned(), authorization, method.to_owned()));
    let origin = request
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("origin"))
                .map(|(_, value)| value.trim())
        })
        .unwrap_or(&source);
    let location = match path {
        _ if method == "OPTIONS" => None,
        "/unsafe-allowed" => Some(format!("{cross}/unsafe-step")),
        "/unsafe-step" => Some(format!("{source}/unsafe-final")),
        "/unsafe-denied" => Some(format!("{cross}/unsafe-blocked")),
        "/same" => Some(format!("{source}/same-final")),
        "/cross" => Some(format!("{cross}/cross-final")),
        "/back" => Some(format!("{cross}/back-step")),
        "/back-step" => Some(format!("{source}/back-final")),
        "/manual" => Some(format!("{source}/manual-final")),
        "/error" => Some(format!("{source}/error-final")),
        _ => None,
    };
    let status = if location.is_some() {
        if path.starts_with("/unsafe-") {
            307
        } else {
            302
        }
    } else {
        200
    };
    let mut headers = location.map_or(String::new(), |url| format!("Location: {url}\r\n"));
    if method == "OPTIONS" {
        headers.push_str("Access-Control-Allow-Methods: PUT\r\n");
        if path != "/unsafe-blocked" {
            headers.push_str("Access-Control-Allow-Headers: x-hop\r\n");
        }
    }
    let body = serde_json::json!({"authorization":authorization,"method":method,"hop":hop,"origin":origin}).to_string();
    let response = format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: {origin}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    socket.write_all(response.as_bytes()).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn redirects_strip_foreign_authorization_and_honor_manual_and_error_modes() {
    let source = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cross = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source_url = format!("http://{}", source.local_addr().unwrap());
    let cross_url = format!("http://{}", cross.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut servers = Vec::new();
    for listener in [source, cross] {
        let source_url = source_url.clone();
        let cross_url = cross_url.clone();
        let requests = requests.clone();
        servers.push(tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                tokio::spawn(reply(
                    socket,
                    source_url.clone(),
                    cross_url.clone(),
                    requests.clone(),
                ));
            }
        }));
    }
    let mut page = PageSession::open(
        include_str!("../../../../harness/detection/fixtures/redirect-policy.html"),
        &format!("{source_url}/"),
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let result = page
        .eval("globalThis.__RESULT=JSON.stringify(__browserFixture)")
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"same":true,"cross":false,"back":false,"manual":{"type":"opaqueredirect","status":0,"ok":false,"url":"/manual","headers":[],"text":""},"errorRejected":true,"unsafeAllowed":true,"unsafeDenied":true})
    );
    let requests = requests.lock().unwrap();
    for path in ["/manual-final", "/error-final"] {
        assert!(
            !requests.iter().any(|(actual, _, _)| actual == path),
            "{requests:?}"
        );
    }
    assert!(requests
        .iter()
        .any(|(path, auth, _)| path == "/same-final" && *auth));
    for path in ["/cross-final", "/back-step", "/back-final"] {
        assert!(
            requests
                .iter()
                .any(|(actual, auth, _)| actual == path && !*auth),
            "{requests:?}"
        );
    }
    for path in ["/unsafe-step", "/unsafe-final", "/unsafe-blocked"] {
        assert!(
            requests
                .iter()
                .any(|(actual, _, method)| actual == path && method == "OPTIONS"),
            "{requests:?}"
        );
    }
    assert!(
        !requests
            .iter()
            .any(|(path, _, method)| path == "/unsafe-blocked" && method != "OPTIONS"),
        "{requests:?}"
    );
    for server in servers {
        server.abort();
    }
}
