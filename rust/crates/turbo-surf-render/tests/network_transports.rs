//! WebSocket, WebSocketStream, EventSource and WebTransport (#144) against the authored
//! Chrome 156 control (harness/detection/fixtures/network-transports-chrome156.json,
//! captured by network-transports-chrome-control.mjs against network-transports-server.mjs).
//! The native side talks to the Rust twin of that server on 127.0.0.1: no live network.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

#[path = "network_transports_server.rs"]
mod server;

const PROBE: &str = include_str!("../../../../harness/detection/network-transports-probe.js");
const CSP_PROBE: &str =
    include_str!("../../../../harness/detection/network-transports-csp-probe.js");
const PAGES: &[(&str, &str)] = &[
    ("/", "<!doctype html><html><head></head><body></body></html>"),
    (
        "/csp",
        "<!doctype html><html><head><meta http-equiv=\"Content-Security-Policy\" content=\"connect-src 'self'\"></head><body></body></html>",
    ),
];

// The saved control is only valid for the exact probe, server and collector bytes.
fn oracle(probe: &str, source: &str) -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/network-transports-chrome156.json"
    ))
    .unwrap();
    assert_eq!(fixture["browser"], "HeadlessChrome/156.0.8078.47");
    assert_eq!(fixture["errors"], json!([]));
    let sha = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    assert_eq!(fixture["fixtures"][probe], sha(source.as_bytes()));
    assert_eq!(
        fixture["serverSha256"],
        sha(include_bytes!(
            "../../../../harness/detection/network-transports-server.mjs"
        ))
    );
    assert_eq!(
        fixture["collectorSha256"],
        sha(include_bytes!(
            "../../../../harness/detection/network-transports-chrome-control.mjs"
        ))
    );
    fixture["result"][probe].clone()
}

async fn observe(probe: &str, path: &str) -> Value {
    let server = server::TransportServer::start(PAGES);
    let html = PAGES.iter().find(|(key, _)| *key == path).unwrap().1;
    let url = format!("http://127.0.0.1:{}{path}", server.port);
    let mut page = PageSession::open(html, &url, "", "", 30000).await.unwrap();
    let raw = page
        .eval_json_expression(probe.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    serde_json::from_str(&raw).unwrap()
}

// Leaf paths where native and Chrome disagree, for readable failures.
fn leaves(path: &str, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            for key in e.keys().chain(a.keys().filter(|key| !e.contains_key(*key))) {
                leaves(
                    &format!("{path}.{key}"),
                    a.get(key).unwrap_or(&Value::Null),
                    e.get(key).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Array(a), Value::Array(e)) if a.len() == e.len() => {
            for (index, (a, e)) in a.iter().zip(e).enumerate() {
                leaves(&format!("{path}[{index}]"), a, e, out);
            }
        }
        _ if actual != expected => out.push(format!("{path}: native={actual} chrome={expected}")),
        _ => {}
    }
}

fn assert_same(actual: &Value, expected: &Value) {
    let mut differences = vec![];
    leaves("", actual, expected, &mut differences);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

fn take(value: &mut Value, section: &str, key: &str) -> Value {
    value[section].as_object_mut().unwrap().remove(key).unwrap()
}

/// Headless Chrome has no language preference; a headed Chrome (the profile this engine
/// presents everywhere else) sends Accept-Language right after Accept-Encoding.
fn with_accept_language(chrome: &Value) -> Value {
    let mut handshakes = chrome.clone();
    for handshake in handshakes.as_array_mut().unwrap() {
        let headers = handshake["headers"].as_array_mut().unwrap();
        let at = headers
            .iter()
            .position(|row| row[0] == "Accept-Encoding")
            .unwrap();
        headers.insert(at + 1, json!(["Accept-Language", "en-US,en;q=0.9"]));
    }
    handshakes
}

/// EventSource requests go through the page's fetch stack, whose header order and client
/// hints are that stack's business; the event-stream contract is these headers.
fn stream_contract(requests: &Value) -> Value {
    const CONTRACT: [&str; 8] = [
        "accept",
        "cache-control",
        "pragma",
        "last-event-id",
        "cookie",
        "sec-fetch-mode",
        "sec-fetch-dest",
        "referer",
    ];
    requests
        .as_array()
        .unwrap()
        .iter()
        .map(|request| {
            let mut headers: Vec<Value> = request["headers"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|row| {
                    let name = row[0].as_str().unwrap().to_ascii_lowercase();
                    CONTRACT
                        .contains(&name.as_str())
                        .then(|| json!([name, row[1]]))
                })
                .collect();
            headers.sort_by_key(|row| row[0].as_str().unwrap().to_owned());
            json!({"path": request["path"], "headers": headers})
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_event_source_and_web_transport_match_chrome() {
    let mut chrome = oracle("network-transports-probe.js", PROBE);
    let mut native = observe(PROBE, "/").await;
    let chrome_handshakes = take(&mut chrome, "webSocket", "handshakes");
    let native_handshakes = take(&mut native, "webSocket", "handshakes");
    let chrome_requests = take(&mut chrome, "eventSource", "requests");
    let native_requests = take(&mut native, "eventSource", "requests");
    assert_same(&native, &chrome);
    assert_same(
        &native_handshakes,
        &with_accept_language(&chrome_handshakes),
    );
    assert_same(
        &stream_contract(&native_requests),
        &stream_contract(&chrome_requests),
    );
}

#[tokio::test(flavor = "current_thread")]
async fn connect_src_governs_websocket_and_event_source_like_chrome() {
    let chrome = oracle("network-transports-csp-probe.js", CSP_PROBE);
    let native = observe(CSP_PROBE, "/csp").await;
    assert_same(&native, &chrome);
}

#[tokio::test(flavor = "current_thread")]
async fn open_transports_never_hold_a_render() {
    let server = server::TransportServer::start(PAGES);
    let script = format!(
        "new WebSocket('ws://127.0.0.1:{0}/ws/echo'); new EventSource('http://127.0.0.1:{0}/sse/forever'); document.body.textContent = 'rendered';",
        server.port
    );
    let html = turbo_surf_render::render_page_with_budget(
        PAGES[0].1,
        &format!("http://127.0.0.1:{}/", server.port),
        &script,
        5000,
    )
    .await
    .unwrap();
    assert!(html.contains("rendered"), "{html}");
}
