//! Document response hooks (lastModified, document.domain sandboxing, agent
//! clustering) and the fullscreen/picture-in-picture policy accessors, against
//! the source-pinned Chrome 156 control over a localhost server.
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const CASES: &str = include_str!("../../../../harness/detection/document-hooks-cases.json");
const PROBE: &str = include_str!("../../../../harness/detection/document-hooks-probe.js");
const REPORT: &str = include_str!("../../../../harness/detection/document-hooks-report.js");
const COLLECTOR: &[u8] = include_bytes!("../../../../harness/detection/document-hooks-chrome.mjs");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/document_hooks_chrome156.json");
const HEADLESS: &str =
    include_str!("../../../../harness/detection/fixtures/document_hooks_headless_chrome156.json");

/// Measured Chrome behavior the engine does not reproduce yet, as `path = engine value`.
///
/// srcdoc and initial about:blank documents share their creator's origin, so in
/// Chrome the srcdoc child's `document.domain = 'b.localhost'` relaxes the top
/// document and the initial child too. The binding records the relaxed
/// domain per Document and offers no host hook to share it.
const KNOWN_GAPS: &[&str] = &[
    "domain-site-keyed/frames/initial/value/domain = \"a.b.localhost\"",
    "domain-site-keyed/top/domain = \"a.b.localhost\"",
];

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    for (name, bytes) in [
        ("casesSha256", CASES.as_bytes()),
        ("probeSha256", PROBE.as_bytes()),
        ("reportSha256", REPORT.as_bytes()),
        ("collectorSha256", COLLECTOR),
    ] {
        assert_eq!(
            control[name],
            format!("{:x}", Sha256::digest(bytes)),
            "stale control: {name}"
        );
    }
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(control["errors"], serde_json::json!([]));
    control
}

/// The case's top host, and the other loopback host its `{{cross}}` children use.
fn hosts(case: &Value) -> (&str, &str) {
    let top = case["host"].as_str().unwrap_or("127.0.0.1");
    (
        top,
        if top == "127.0.0.1" {
            "localhost"
        } else {
            "127.0.0.1"
        },
    )
}

fn headers(value: &Value) -> Vec<(String, String)> {
    serde_json::from_value(value.clone()).unwrap()
}

/// Serves each case under `/<index>/` exactly as the Chrome collector does.
async fn serve(cases: Value) -> u16 {
    serve_delayed(cases, &[]).await
}

/// `serve`, holding each `(resource, ms)` response back to stand in for host load.
async fn serve_delayed(cases: Value, delays: &'static [(&'static str, u64)]) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cases = std::sync::Arc::new(cases);
    let served = std::sync::Arc::new((
        std::sync::Mutex::new(std::collections::HashSet::<String>::new()),
        tokio::sync::Notify::new(),
    ));
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let cases = cases.clone();
            let served = served.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut chunk = [0; 1024];
                    let size = socket.read(&mut chunk).await.unwrap_or(0);
                    if size == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..size]);
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap();
                if let Some((_, ms)) = delays.iter().find(|(name, _)| path.ends_with(name)) {
                    tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                }
                // Chrome's control committed the cross-origin children in document order,
                // and an origin's agent-cluster keying follows its first committed document
                // (turbo-surf#102): serve them in that order, whatever the host's load.
                let earlier = earlier_cross_children(&cases, path);
                let wait = async {
                    loop {
                        let notified = served.1.notified();
                        if earlier
                            .iter()
                            .all(|key| served.0.lock().unwrap().contains(key))
                        {
                            break;
                        }
                        notified.await;
                    }
                };
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), wait).await;
                let (status, fields, body) = respond(&cases, path, port);
                let mut head = format!("HTTP/1.1 {status}\r\nCache-Control: no-store\r\n");
                for (name, value) in fields {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str(&format!(
                    "Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                ));
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(body.as_bytes()).await;
                served.0.lock().unwrap().insert(path.to_string());
                served.1.notify_waiters();
            });
        }
    });
    port
}

/// The `/<index>/<name>` paths of the case's cross-origin children before `path`'s, in
/// document order (empty when `path` is not one of them).
fn earlier_cross_children(cases: &Value, path: &str) -> Vec<String> {
    let Some((index, name)) = path.strip_prefix('/').and_then(|rest| rest.split_once('/')) else {
        return Vec::new();
    };
    let Some(html) = index
        .parse::<usize>()
        .ok()
        .and_then(|index| cases.get(index))
        .and_then(|case| case["html"].as_str())
    else {
        return Vec::new();
    };
    let mut order: Vec<&str> = Vec::new();
    for part in html.split("{{cross}}").skip(1) {
        let child = part.split(['?', '\'', '"']).next().unwrap_or("");
        if !order.contains(&child) {
            order.push(child);
        }
    }
    match order.iter().position(|child| *child == name) {
        Some(position) => order[..position]
            .iter()
            .map(|child| format!("/{index}/{child}"))
            .collect(),
        None => Vec::new(),
    }
}

fn respond(cases: &Value, path: &str, port: u16) -> (&'static str, Vec<(String, String)>, String) {
    let missing = ("404 Not Found", Vec::new(), String::new());
    if path == "/report.js" {
        let fields = vec![("Content-Type".into(), "text/javascript".into())];
        return ("200 OK", fields, REPORT.into());
    }
    let Some((index, name)) = path.strip_prefix('/').and_then(|rest| rest.split_once('/')) else {
        return missing;
    };
    let Some(case) = index
        .parse::<usize>()
        .ok()
        .and_then(|index| cases.get(index))
    else {
        return missing;
    };
    let resource = if name.is_empty() {
        case
    } else {
        &case["resources"][name]
    };
    let Some(html) = resource["html"].as_str() else {
        return missing;
    };
    let mut fields = vec![("Content-Type".to_string(), "text/html".to_string())];
    fields.extend(headers(&resource["headers"]));
    let cross = format!("http://{}:{port}/{index}/", hosts(case).1);
    ("200 OK", fields, html.replace("{{cross}}", &cross))
}

/// JSON-pointer paths where `actual` departs from Chrome, with the engine's value.
fn divergences(path: String, actual: &Value, expected: &Value, out: &mut Vec<(String, Value)>) {
    match (actual, expected) {
        (Value::Object(left), Value::Object(right)) => {
            let mut keys: Vec<_> = left.keys().chain(right.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let missing = Value::String("<absent>".into());
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
        _ if actual != expected => out.push((path, actual.clone())),
        _ => {}
    }
}

/// Runs one case against `port` and returns its divergences from the control.
async fn case_divergences(
    cases: &Value,
    expected: &[Value],
    index: usize,
    port: u16,
) -> Vec<(String, Value)> {
    let case = &cases[index];
    let expected = &expected[index];
    assert_eq!(case["id"], expected["id"]);
    let (top, cross) = hosts(case);
    let html = case["html"]
        .as_str()
        .unwrap()
        .replace("{{cross}}", &format!("http://{cross}:{port}/{index}/"));
    let mut page = PageSession::open_with_response_headers(
        &html,
        &format!("http://{top}:{port}/{index}/"),
        "",
        "",
        10000,
        "",
        Default::default(),
        &headers(&case["headers"]),
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let mut found = Vec::new();
    divergences(
        case["id"].as_str().unwrap().to_string(),
        &actual,
        &expected["result"],
        &mut found,
    );
    found
}

/// turbo-surf#102: an origin's agent-cluster keying is fixed by its first committed
/// document (Chrome 156 commits `cross` before `crossoff` in the control, and keys both by
/// the first; with `cross` held back Chrome keys both like `crossoff`). The control was
/// measured with the children committing in document order, so the comparison must not
/// depend on which loopback response the host happens to finish first under load.
#[tokio::test(flavor = "current_thread")]
async fn agent_cluster_children_compare_independently_of_response_timing() {
    let control = control();
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let expected = control["result"].as_array().unwrap();
    let index = cases
        .as_array()
        .unwrap()
        .iter()
        .position(|case| case["id"] == "agent-cluster-site-keyed-children")
        .unwrap();
    // A loaded host finishing the first cross-origin child after the second.
    let port = serve_delayed(cases.clone(), &[("/report", 400)]).await;
    assert_eq!(
        case_divergences(&cases, expected, index, port).await,
        Vec::<(String, Value)>::new()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn document_response_hooks_and_fullscreen_policy_match_chrome156() {
    let control = control();
    // Headless and headful Chrome agree on every probed value, including
    // pictureInPictureEnabled: it reflects policy, not a visible window.
    let headless: Value = serde_json::from_str(HEADLESS).unwrap();
    assert_eq!(headless["headless"], true);
    assert_eq!(control["headless"], false);
    assert_eq!(headless["probeSha256"], control["probeSha256"]);
    assert_eq!(headless["result"], control["result"]);
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let expected = control["result"].as_array().unwrap();
    assert_eq!(cases.as_array().unwrap().len(), expected.len());
    let port = serve(cases.clone()).await;
    let mut found = Vec::new();
    for index in 0..expected.len() {
        found.extend(case_divergences(&cases, expected, index, port).await);
    }
    let found: Vec<_> = found
        .iter()
        .map(|(path, value)| format!("{path} = {value}"))
        .collect();
    assert_eq!(found, KNOWN_GAPS, "divergences from the Chrome control");
}
