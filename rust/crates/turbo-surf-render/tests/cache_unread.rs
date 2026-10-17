use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

async fn evaluate(page: &mut PageSession, expression: &str) -> String {
    page.eval_expression(expression).await.unwrap()
}
async fn check(page: &mut PageSession, url: &str) -> Value {
    serde_json::from_str(
        &evaluate(
            page,
            &format!(
                "(async()=>JSON.stringify(await unreadControl.check({})))()",
                serde_json::json!(url)
            ),
        )
        .await,
    )
    .unwrap()
}
fn collect(page: &mut PageSession) {
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
}
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
}

#[tokio::test(flavor = "current_thread")]
async fn unread_cache_completion_collection_and_cancellation_match_chrome() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let closed = Arc::new(Mutex::new(BTreeMap::<String, bool>::new()));
    let closures = closed.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let closed = closures.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let count = socket.read(&mut bytes).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&bytes[..count]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&request);
                let path = head.split_whitespace().nth(1).unwrap().to_string();
                let target = url::Url::parse(&format!("http://localhost{path}")).unwrap();
                let query: BTreeMap<_, _> = target.query_pairs().into_owned().collect();
                if target.path() == "/unread-frame" {
                    let body=b"<!doctype html><script>(async()=>{await fetch('/unread/retire');top.postMessage({kind:'unread-frame'},'*')})()</script>";
                    let mut response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
                    response.extend(body);
                    let _ = socket.write_all(&response).await;
                    return;
                }
                if target.path() == "/unread-hold" {
                    let _=socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n").await;
                    let mut bytes = [0; 1];
                    let eof = matches!(socket.read(&mut bytes).await, Ok(0));
                    closed.lock().unwrap().insert(path, eof);
                    return;
                }
                let size = query
                    .get("bytes")
                    .map_or(16384, |s| s.parse::<usize>().unwrap());
                let control = query.get("control").map_or("max-age=3600", String::as_str);
                let fields = if control == "absent" {
                    String::new()
                } else {
                    format!("Cache-Control: {control}\r\n")
                };
                if socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nContent-Type: text/plain\r\n{fields}Connection: close\r\n\r\na").as_bytes()).await.is_err(){return;}
                let mut byte = [0; 1];
                let canceled = tokio::select! { biased; read=socket.read(&mut byte)=>matches!(read,Ok(0)),_ = tokio::time::sleep(std::time::Duration::from_millis(200))=>false };
                if !canceled {
                    let _ = socket.write_all(&vec![b'a'; size - 1]).await;
                }
                closed.lock().unwrap().insert(path, canceled);
            });
        }
    });
    let mut page = PageSession::open("<!doctype html><body>", &format!("{origin}/"), "", "", 5000)
        .await
        .unwrap();
    evaluate(
        &mut page,
        include_str!("../../../../harness/detection/cache-unread-probe.js"),
    )
    .await;
    let mut result = Map::new();
    evaluate(
        &mut page,
        "(async()=>{globalThis.unreadHeld=await fetch('/unread/held')})()",
    )
    .await;
    settle().await;
    result.insert("held".into(), check(&mut page, "/unread/held").await);
    evaluate(&mut page, "(async()=>{await fetch('/unread/gc')})()").await;
    collect(&mut page);
    settle().await;
    result.insert("gc".into(), check(&mut page, "/unread/gc").await);
    evaluate(
        &mut page,
        "(async()=>{const response=await fetch('/unread/cancel');await response.body.cancel()})()",
    )
    .await;
    settle().await;
    result.insert("cancel".into(), check(&mut page, "/unread/cancel").await);
    evaluate(&mut page,"(async()=>{const controller=new AbortController();await fetch('/unread/abort',{signal:controller.signal});controller.abort()})()").await;
    settle().await;
    result.insert("abort".into(), check(&mut page, "/unread/abort").await);
    evaluate(&mut page, "unreadControl.retire()").await;
    settle().await;
    result.insert("retire".into(), check(&mut page, "/unread/retire").await);
    for size in [65536, 262144, 1048576, 2097152] {
        for kind in ["held", "gc"] {
            let url = format!("/unread/size-{kind}?bytes={size}");
            let assignment = if kind == "held" {
                "globalThis.unreadHeld="
            } else {
                ""
            };
            evaluate(
                &mut page,
                &format!(
                    "(async()=>{{{assignment}await fetch({});}})()",
                    serde_json::json!(url)
                ),
            )
            .await;
            if kind == "gc" {
                collect(&mut page);
            }
            settle().await;
            result.insert(format!("{kind}-{size}"), check(&mut page, &url).await);
        }
    }
    for control in ["absent", "no-cache", "no-store"] {
        let url = format!("/unread/control?control={control}");
        evaluate(
            &mut page,
            &format!("(async()=>{{await fetch({})}})()", serde_json::json!(url)),
        )
        .await;
        collect(&mut page);
        settle().await;
        result.insert(format!("gc-{control}"), check(&mut page, &url).await);
    }
    evaluate(&mut page, "(async()=>{await fetch('/unread-hold')})()").await;
    collect(&mut page);
    settle().await;
    result.insert(
        "holdClosed".into(),
        serde_json::json!(if closed
            .lock()
            .unwrap()
            .get("/unread-hold")
            .copied()
            .unwrap_or(false)
        {
            1
        } else {
            0
        }),
    );
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/cache-unread-chrome155.json"
    ))
    .unwrap();
    assert_eq!(Value::Object(result), expected["result"]);
    assert!(closed.lock().unwrap()["/unread/cancel"]);
    assert!(closed.lock().unwrap()["/unread/abort"]);
    assert!(closed.lock().unwrap()["/unread/retire"]);
    let logs: Vec<Value> = serde_json::from_str(&page.drain_network_log()).unwrap();
    for path in ["/unread/held", "/unread/gc"] {
        let log = logs
            .iter()
            .find(|row| row["url"] == format!("{origin}{path}"))
            .unwrap();
        assert_eq!(log["body"].as_str().unwrap().len(), 16384);
        assert_eq!(log["bodyIncomplete"], false);
    }
    let held = logs
        .iter()
        .find(|row| row["url"] == format!("{origin}/unread-hold"))
        .unwrap();
    assert_eq!(held["bodyIncomplete"], true);
    // A task owns only native bytes and a watch receiver. Isolate disposal closes
    // its sole creation-owner sender, even after the body wrapper was collected.
    page.close();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !closed
            .lock()
            .unwrap()
            .get("/unread-hold")
            .copied()
            .unwrap_or(false)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("isolate disposal must close detached cache fills");
    server.abort();
}
