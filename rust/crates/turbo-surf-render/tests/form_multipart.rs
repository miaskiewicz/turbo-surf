//! multipart/form-data wire bytes (fetch FormData bodies, FormData from a form with
//! file controls, and the native form submission) against the Chrome 156 control,
//! for a UTF-8 and a windows-1252 document. Boundaries are random in both engines.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/form-multipart-probe.js");
const COLLECTOR: &[u8] = include_bytes!("../../../../harness/detection/form-multipart-chrome.mjs");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/form_multipart_chrome156.json");

/// Echo every /wire request as Chrome's collector does, keeping the submissions.
fn server() -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let submissions = Arc::new(Mutex::new(Vec::new()));
    let seen = submissions.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            let (head, body) = loop {
                let count = stream.read(&mut chunk).unwrap_or(0);
                if count == 0 {
                    break (String::new(), Vec::new());
                }
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break (head, bytes[end + 4..end + 4 + length].to_vec());
                    }
                }
            };
            let mut lines = head.lines();
            let request = lines.next().unwrap_or_default().to_owned();
            let content_type = lines.find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-type")
                    .then(|| value.trim().to_owned())
            });
            let wire = serde_json::json!({
                "method": request.split(' ').next().unwrap_or_default(),
                "contentType": content_type,
                "bodyHex": body.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            });
            let (kind, response) = if request.contains("kind=submit") {
                seen.lock().unwrap().push(wire.clone());
                ("text/html; charset=utf-8", format!("<body>{wire}</body>"))
            } else {
                ("application/json", wire.to_string())
            };
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                )
                .as_bytes(),
            );
        }
    });
    (port, submissions)
}

/// The wire with its random boundary replaced, after checking Blink's boundary shape.
fn normalized(wire: &Value) -> Value {
    let content_type = wire["contentType"].as_str().unwrap();
    let boundary = content_type
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap();
    let token = boundary.strip_prefix("----WebKitFormBoundary").unwrap();
    assert!(
        token.len() == 16 && token.bytes().all(|byte| byte.is_ascii_alphanumeric()),
        "{boundary}"
    );
    let hex: String = boundary.bytes().map(|byte| format!("{byte:02x}")).collect();
    let body = wire["bodyHex"]
        .as_str()
        .unwrap()
        .replace(&hex, "<boundary>");
    serde_json::json!({"method": wire["method"], "body": body})
}

#[tokio::test(flavor = "current_thread")]
async fn multipart_bodies_match_chrome156_wire_bytes() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    assert_eq!(
        control["collectorSha256"],
        format!("{:x}", Sha256::digest(COLLECTOR))
    );
    let (port, submissions) = server();
    for (index, case) in control["result"].as_array().unwrap().iter().enumerate() {
        let charset = case["id"].as_str().unwrap();
        let headers = vec![(
            "content-type".to_owned(),
            format!("text/html; charset={charset}"),
        )];
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><html><head></head><body></body></html>",
            &format!("http://127.0.0.1:{port}/{index}"),
            "",
            "",
            20000,
            "globalThis.__wireSelf = true;",
            ClientHints::default(),
            &headers,
        )
        .await
        .unwrap();
        // The form's encoding is the response's charset (document.characterSet is not
        // derived from it in this engine, so it is not compared here).
        let raw = page
            .eval_json_expression(PROBE.trim().trim_end_matches(';'))
            .await
            .unwrap();
        page.close();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        for kind in ["fetch", "formData"] {
            assert_eq!(
                normalized(&actual[kind]),
                normalized(&case["result"][kind]),
                "{charset} {kind}"
            );
        }
        let submitted = submissions.lock().unwrap().pop().expect("form submission");
        assert_eq!(
            normalized(&submitted),
            normalized(&case["result"]["submit"]),
            "{charset} submit"
        );
    }
}
