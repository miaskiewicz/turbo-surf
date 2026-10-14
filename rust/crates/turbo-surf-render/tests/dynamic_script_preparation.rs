//! Runtime consumers must match real mutation triggers, not repeated DOM polling.
use serde_json::Value;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_dynamic_script_triggers_and_current_script_match_chrome() {
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "http://localhost/authored",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/dynamic-script-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page
        .eval(&format!("globalThis.__RESULT=JSON.stringify({probe})"))
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/dynamic-script-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, expected["result"]);
    let next = page
        .eval("globalThis.__RESULT=JSON.stringify(__dynamicMarks)")
        .await
        .unwrap();
    let marks: Value = serde_json::from_str(&next).unwrap();
    assert_eq!(
        marks,
        serde_json::json!([
            "immediate",
            "late-text",
            "data-block",
            "private-preparation",
            "shadow-null"
        ]),
        "later hydration must not revive CharacterData or innerHTML scripts"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn ordered_dynamic_classics_fetch_concurrently_and_scope_current_script_to_execution() {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let gate = gate.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.windows(4).any(|row| row == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap();
                let id = path.strip_prefix('/').unwrap().strip_suffix(".js").unwrap();
                log.lock().unwrap().push(id.to_string());
                if id == "slow" {
                    gate.acquire().await.unwrap().forget();
                } else {
                    gate.add_permits(1);
                }
                let body = format!(
                    "__orderedScriptRows.push('execute:{id}:' + document.currentScript.id);"
                );
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let mut page = PageSession::open(
        "<!doctype html><head></head><body></body>",
        &origin,
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/dynamic-script-ordering-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page
        .eval(&format!(
            "(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})()"
        ))
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/dynamic-script-ordering-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, expected["result"]);
    assert_eq!(
        serde_json::json!(*requests.lock().unwrap()),
        expected["requests"]
    );
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn retired_document_script_preparation_stays_inert_after_adoption() {
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "http://localhost/authored",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/retired-script-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page
        .eval(&format!(
            "(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})()"
        ))
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/retired-script-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, expected["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_inline_script_restores_current_script_before_session_reuse() {
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "http://localhost/authored",
        "",
        "",
        100,
    )
    .await
    .unwrap();
    let interrupted=page.eval_expression("(()=>{const script=document.createElement('script');script.textContent='while(true){}';document.body.appendChild(script);return 'unexpected';})()").await;
    assert!(
        interrupted.is_err(),
        "the runaway script must be interrupted: {interrupted:?}"
    );
    let next = page
        .eval_json_expression(
            "({current:document.currentScript===null,body:document.body.localName})",
        )
        .await
        .unwrap();
    let next: Value = serde_json::from_str(&next).unwrap();
    assert_eq!(next, serde_json::json!({"current":true,"body":"body"}));
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_queued_script_releases_completion_before_session_reuse() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..count]);
                    if request.windows(4).any(|row| row == b"\r\n\r\n") {
                        break;
                    }
                }
                let body = "globalThis.__queuedEntered=true;while(true){}";
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let mut page = PageSession::open("<!doctype html><body></body>", &origin, "", "", 100)
        .await
        .unwrap();
    let interrupted = page.eval_expression("(()=>{const script=document.createElement('script');script.src='/runaway.js';document.body.appendChild(script);return new Promise(()=>{});})()").await;
    assert!(
        interrupted.is_err(),
        "queued execution must be interrupted: {interrupted:?}"
    );
    let next = page.eval_json_expression("({current:document.currentScript===null,entered:__queuedEntered,body:document.body.localName})").await.unwrap();
    let next: Value = serde_json::from_str(&next).unwrap();
    assert_eq!(
        next,
        serde_json::json!({"current":true,"entered":true,"body":"body"})
    );
    server.abort();
}
