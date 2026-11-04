//! Native form navigation submitted inside child frames (encodings, navigation
//! request headers, target routing, sandbox allow-forms, form-action) against the
//! Chrome 156 control, over a localhost server mirroring the control's `/echo`.
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const PROBE: &str =
    include_str!("../../../../harness/detection/child-frame-form-navigation-probe.js");
const CONTROL: &str = include_str!(
    "../../../../harness/detection/fixtures/child-frame-form-navigation-chrome156.json"
);

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .ends_with("Chrome/156.0.8078.47"));
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(
        control["fixtures"]["child-frame-form-navigation-probe.js"],
        format!("{:x}", Sha256::digest(PROBE)),
        "stale control"
    );
    control["result"]["child-frame-form-navigation-probe.js"].clone()
}

/// JSON-pointer paths where `actual` departs from Chrome, with the engine's value.
fn divergences(path: String, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Object(left), Value::Object(right)) => {
            let mut keys: Vec<_> = left.keys().chain(right.keys()).collect();
            keys.sort();
            keys.dedup();
            let missing = Value::String("<absent>".into());
            for key in keys {
                divergences(
                    format!("{path}/{key}"),
                    left.get(key).unwrap_or(&missing),
                    right.get(key).unwrap_or(&missing),
                    out,
                );
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                divergences(format!("{path}/{index}"), left, right, out);
            }
        }
        _ if actual != expected => out.push(format!("{path} = {actual}")),
        _ => {}
    }
}

const ECHO_HEADERS: [&str; 10] = [
    "content-type",
    "content-length",
    "origin",
    "referer",
    "cache-control",
    "upgrade-insecure-requests",
    "sec-fetch-dest",
    "sec-fetch-mode",
    "sec-fetch-site",
    "sec-fetch-user",
];

/// The control server's routes: `/echo…` returns the request it received and
/// `/csp-…` pages carry `form-action 'none'`.
fn respond(head: &str, body: &[u8], port: u16) -> String {
    let mut lines = head.split("\r\n");
    let mut start = lines.next().unwrap_or("").split(' ');
    let method = start.next().unwrap_or("").to_string();
    let path = start.next().unwrap_or("/").to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    let origin = format!("http://127.0.0.1:{port}");
    if path.starts_with("/echo") {
        let boundary = headers
            .iter()
            .find(|(name, _)| name == "content-type")
            .and_then(|(_, value)| value.split_once("boundary="))
            .map(|(_, boundary)| boundary.to_string());
        let scrub = |text: &str| {
            let text = text.replace(&origin, "<origin>");
            boundary.as_ref().map_or(text.clone(), |boundary| {
                text.replace(boundary, "<boundary>")
            })
        };
        let mut echoed = serde_json::Map::new();
        for name in ECHO_HEADERS {
            if let Some((_, value)) = headers.iter().find(|(header, _)| header == name) {
                echoed.insert(name.into(), scrub(value).into());
            }
        }
        let body: String = body.iter().map(|byte| char::from(*byte)).collect();
        let echo = serde_json::json!({"method": method, "url": path, "headers": echoed, "body": scrub(&body)});
        let text = echo.to_string().replace('&', "&amp;").replace('<', "&lt;");
        return format!(
            "<!doctype html><html><head></head><body><pre id=\"echo\">{text}</pre></body></html>"
        );
    }
    let policy = if path.starts_with("/csp-") {
        "<meta http-equiv=\"Content-Security-Policy\" content=\"form-action 'none'\">"
    } else {
        ""
    };
    format!("<!doctype html><html><head>{policy}</head><body></body></html>")
}

async fn serve(listener: tokio::net::TcpListener, port: u16) {
    while let Ok((mut socket, _)) = listener.accept().await {
        tokio::spawn(async move {
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            let split = loop {
                if let Some(at) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break at;
                }
                let size = socket.read(&mut chunk).await.unwrap_or(0);
                if size == 0 {
                    return;
                }
                request.extend_from_slice(&chunk[..size]);
            };
            let head = String::from_utf8_lossy(&request[..split]).to_string();
            let length = head
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while request.len() < split + 4 + length {
                let size = socket.read(&mut chunk).await.unwrap_or(0);
                if size == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..size]);
            }
            let end = request.len().min(split + 4 + length);
            let html = respond(&head, &request[split + 4..end], port);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                html.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
    }
}

#[tokio::test(flavor = "current_thread")]
async fn child_frame_form_navigation_matches_chrome156() {
    let expected = control();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(serve(listener, port));
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("http://127.0.0.1:{port}/"),
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let value: Value = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
    let mut found = Vec::new();
    divergences(String::new(), &actual, &expected, &mut found);
    assert_eq!(
        found,
        Vec::<String>::new(),
        "divergences from the Chrome control"
    );
}
