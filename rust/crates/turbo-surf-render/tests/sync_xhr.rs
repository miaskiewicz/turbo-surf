//! Synchronous XMLHttpRequest on a window (turbo-surf#138) against the Chrome 156
//! control: blocking http, blob: and data: loads, the event order, the window-only
//! responseType/timeout restrictions and NetworkError from send(). The page is
//! served by a localhost thread, so the blocked event loop cannot starve it.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/sync-xhr-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/sync-xhr-chrome156.json");
const DOCUMENT: &str = "<!doctype html><html><head></head><body></body></html>";

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert_eq!(control["browser"], "HeadlessChrome/156.0.8078.47");
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE)),
        "stale control"
    );
    serde_json::from_str(control["result"].as_str().unwrap()).unwrap()
}

/// Serves the probe document (Chrome's recorder serves the same bytes) on every path.
fn serve() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => request.extend_from_slice(&chunk[..count]),
                }
            }
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{DOCUMENT}",
                    DOCUMENT.len()
                )
                .as_bytes(),
            );
        }
    });
    format!("http://{address}/")
}

#[tokio::test(flavor = "current_thread")]
async fn synchronous_xhr_matches_chrome156() {
    let expected = control();
    let url = serve();
    let mut page = PageSession::open(DOCUMENT, &url, "", "", 20000)
        .await
        .unwrap();
    let raw = page
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let value: Value = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
    let fields = expected.as_object().unwrap();
    assert_eq!(actual.as_object().unwrap().len(), fields.len());
    for (field, chrome) in fields {
        assert_eq!(&actual[field], chrome, "{field}");
    }
}
