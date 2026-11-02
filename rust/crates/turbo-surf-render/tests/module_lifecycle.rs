//! Explicit module graphs share the offline Chrome fixture and real network gates.
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;
const HTML: &str = include_str!("../../../../harness/detection/fixtures/modules.html");
const ENTRY: &str = include_str!("../../../../harness/detection/fixtures/modules/entry.mjs");
const DEP: &str = include_str!("../../../../harness/detection/fixtures/modules/dep.mjs");
const ASYNC: &str = include_str!("../../../../harness/detection/fixtures/modules/async.mjs");
const LATE: &str = include_str!("../../../../harness/detection/fixtures/modules/late-dep.mjs");
#[tokio::test(flavor = "current_thread")]
async fn explicit_modules_link_real_sources_and_async_graph_does_not_block_dcl() {
    let (base, server, paths) = serve_modules().await;
    let mut page = PageSession::open(HTML, &base, "", "", 60_000)
        .await
        .unwrap();
    let result=page.eval("globalThis.__RESULT=JSON.stringify({...__browserFixture,wrongMime:globalThis.wrongMimeExecuted===true})").await.unwrap();
    server.abort();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value["dcl"],
        serde_json::json!({"deferred":true,"async":false,"currentScript":true}),
        "{value}"
    );
    for (name, kind) in [
        ("deferred", "load"),
        ("async", "load"),
        ("missing", "error"),
        ("mime", "error"),
    ] {
        assert_eq!(
            value["scriptEvents"][name],
            serde_json::json!([{"type":kind,"trusted":true,"target":true,"currentScript":true}]),
            "{value}"
        );
    }
    assert!(value["scriptEvents"].get("inline").is_none(), "{value}");
    assert_eq!(value["inline"], true, "{value}");
    assert_eq!(
        value["inlineSources"],
        serde_json::json!([
          {"url":cfg!(turbo_surf_native_proxy),"stack":cfg!(turbo_surf_native_proxy),"local":"independent"},
          {"url":cfg!(turbo_surf_native_proxy),"stack":cfg!(turbo_surf_native_proxy),"local":"independent"}
        ]),
        "{value}"
    );
    assert_eq!(value["wrongMime"], false, "{value}");
    assert_eq!(
        value["deferred"],
        serde_json::json!({"value":"linked","source":{"url":"/dep.mjs","currentScript":true,"stack":true},"entry":"/entry.mjs","currentScript":true,"ready":"interactive"}),
        "{value}"
    );
    assert_eq!(
        value["async"],
        serde_json::json!({"value":"released-after-dcl","currentScript":true,"ready":"interactive"}),
        "{value}"
    );
    let events = value["events"].as_array().unwrap();
    let position = |name| events.iter().position(|value| value == name).unwrap();
    assert!(position("classic:loading") < position("deferred:eval"));
    assert!(position("deferred:eval") < position("dcl"));
    assert!(position("dcl") < position("async:eval"));
    assert!(position("async:eval") < position("window:load"));
    for path in [
        "/entry.mjs",
        "/dep.mjs",
        "/async.mjs",
        "/late-dep.mjs",
        "/module-gate",
    ] {
        assert_eq!(
            paths
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.as_str() == path)
                .count(),
            1,
            "{paths:?}"
        );
    }
}

async fn serve_modules() -> (
    String,
    tokio::task::JoinHandle<()>,
    Arc<std::sync::Mutex<Vec<String>>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let paths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests = paths.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let gate = gate.clone();
            let requests = requests.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 4096];
                    let count = socket.read(&mut bytes).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&bytes[..count]);
                    if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                requests.lock().unwrap().push(path.to_string());
                if path == "/module-gate" || path == "/evaluation-gate" {
                    gate.add_permits(1);
                }
                if path == "/late-dep.mjs" {
                    gate.acquire().await.unwrap().forget();
                }
                if path == "/await-gate" {
                    gate.acquire().await.unwrap().forget();
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                let (status, mime, body) = match path {
                    "/tla.mjs" => (
                        200,
                        "application/javascript",
                        include_str!("../../../../harness/detection/fixtures/modules/tla.mjs"),
                    ),
                    "/runtime-error.mjs" => (
                        200,
                        "application/javascript",
                        include_str!(
                            "../../../../harness/detection/fixtures/modules/runtime-error.mjs"
                        ),
                    ),
                    "/parse-error.mjs" => (
                        200,
                        "application/javascript",
                        include_str!(
                            "../../../../harness/detection/fixtures/modules/parse-error.source"
                        ),
                    ),
                    "/repeat.mjs" => (
                        200,
                        "application/javascript",
                        include_str!("../../../../harness/detection/fixtures/modules/repeat.mjs"),
                    ),
                    "/await-gate" | "/evaluation-gate" => (200, "text/plain", "ready"),
                    "/entry.mjs" => (200, "application/javascript", ENTRY),
                    "/dep.mjs" => (200, "application/javascript", DEP),
                    "/async.mjs" => (200, "application/javascript", ASYNC),
                    "/late-dep.mjs" => (200, "application/javascript", LATE),
                    "/wrong-mime.mjs" => (200, "text/plain", "globalThis.wrongMimeExecuted=true;"),
                    "/module-gate" => (200, "text/plain", "ready"),
                    _ => (404, "application/javascript", "missing"),
                };
                socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    (base, server, paths)
}

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn module_evaluation_releases_load_and_dcl_before_top_level_await_and_reports_window_errors()
{
    let (base, server, paths) = serve_modules().await;
    let html = include_str!("../../../../harness/detection/fixtures/modules-evaluation.html");
    let mut page = PageSession::open(html, &base, "", "", 60_000)
        .await
        .unwrap();
    let result = page
        .eval("globalThis.__RESULT=JSON.stringify(__browserFixture)")
        .await
        .unwrap();
    server.abort();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    for stage in ["dcl", "load"] {
        assert_eq!(
            value[stage],
            serde_json::json!({"started":true,"done":false,"currentScript":true}),
            "{value}"
        );
    }
    assert_eq!(
        value["tla"],
        serde_json::json!({"currentScript":true,"readyState":"complete","url":"/tla.mjs"}),
        "{value}"
    );
    assert_eq!(value["repeat"], 1, "{value}");
    for name in ["tla", "runtime", "parse", "repeat1", "repeat2"] {
        assert_eq!(
            value["scriptEvents"][name],
            serde_json::json!([{"type":"load","trusted":true,"target":true,"currentScript":true}]),
            "{value}"
        );
    }
    let errors = value["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "{value}");
    for (name, source) in [
        ("RangeError", "/runtime-error.mjs"),
        ("SyntaxError", "/parse-error.mjs"),
    ] {
        let event = errors.iter().find(|event| event["name"] == name).unwrap();
        assert_eq!(event["filename"], source, "{value}");
        assert_eq!(event["brand"], "[object ErrorEvent]", "{value}");
        for field in ["trusted", "target", "currentScript", "line"] {
            assert_eq!(event[field], true, "{value}");
        }
    }
    let events = value["events"].as_array().unwrap();
    let position = |name| events.iter().position(|value| value == name).unwrap();
    assert!(position("tla:start") < position("tla:load"));
    assert!(position("tla:load") < position("dcl"));
    assert!(position("dcl") < position("window:load"));
    assert!(position("window:load") < position("tla:end"));
    assert_eq!(
        paths
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.as_str() == "/repeat.mjs")
            .count(),
        1
    );
}
