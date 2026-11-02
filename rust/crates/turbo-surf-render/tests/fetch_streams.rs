use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn official_stream_abort_interfaces_match_source_pinned_chrome() {
    let mut page = PageSession::open(
        "<!doctype html><body>Authored stream control</body>",
        "http://127.0.0.1:9988/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/stream-abort-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-abort-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, expected["result"]);
}

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn live_ancestor_signal_does_not_root_retired_dependent_realms() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://stream-lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(r#"(()=>{
      globalThis.liveAncestor=new AbortController();
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const signal=child.AbortSignal.any([liveAncestor.signal]);signal.addEventListener('abort',()=>{});
      globalThis.webRealmRefs=[new WeakRef(child),new WeakRef(signal)];frame.remove();return 'prepared';
    })()"#).await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression(
            "JSON.stringify(webRealmRefs.map(reference=>reference.deref()===undefined))"
        )
        .await
        .unwrap(),
        "[true,true]"
    );
    assert_eq!(
        page.eval_expression(
            "liveAncestor.abort('after-collection');String(liveAncestor.signal.aborted)"
        )
        .await
        .unwrap(),
        "true"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_fetch_headers_body_cancellation_and_binary_upload_match_chrome() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let closed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let closed_server = closed.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let closed = closed_server.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let head;
                loop {
                    let mut bytes = [0; 1024];
                    let count = socket.read(&mut bytes).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&bytes[..count]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        head = end + 4;
                        break;
                    }
                }
                let header = String::from_utf8_lossy(&request[..head]);
                let path = header.split_whitespace().nth(1).unwrap().to_string();
                let length = header
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map_or(0, |(_, value)| value.trim().parse::<usize>().unwrap());
                if path == "/hold" || path == "/pending" {
                    if path == "/hold" {
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n").await.unwrap();
                    }
                    let mut bytes = [0; 1];
                    if matches!(socket.read(&mut bytes).await, Ok(0)) {
                        closed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    return;
                }
                while request.len() - head < length {
                    let mut bytes = [0; 1024];
                    let count = socket.read(&mut bytes).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&bytes[..count]);
                }
                let (status, content, body) = match path.as_str() {
                    "/binary" => (
                        "200 Authored",
                        "application/octet-stream",
                        vec![0, 255, 128, 65, 226, 130, 172],
                    ),
                    "/echo" => (
                        "200 OK",
                        "application/json",
                        serde_json::to_vec(&request[head..head + length]).unwrap(),
                    ),
                    "/null" => ("204 No Content", "text/plain", Vec::new()),
                    _ => ("404 Not Found", "text/plain", Vec::new()),
                };
                let mut response=format!("HTTP/1.1 {status}\r\nContent-Type: {content}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
                response.extend_from_slice(&body);
                let _ = socket.write_all(&response).await;
            });
        }
    });
    let mut page = PageSession::open(
        "<!doctype html><body>Authored fetch control</body>",
        &format!("{origin}/"),
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/fetch-stream-probe.js")
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
        "../../../../harness/detection/fixtures/fetch-stream-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, expected["result"]);
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while closed.load(std::sync::atomic::Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Both unread native body transports must close on cancellation");
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn collecting_a_no_store_body_closes_its_native_transport() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).await.unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&bytes[..count]);
            if request.windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: 8\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut bytes = [0; 1];
        socket.read(&mut bytes).await.unwrap()
    });
    let mut page = PageSession::open(
        "<!doctype html><body>",
        &format!("{origin}/"),
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    assert_eq!(page.eval_expression("(async()=>{const response=await fetch('/hold');globalThis.unconsumedBody=new WeakRef(response.body);return 'headers';})()").await.unwrap(),"headers");
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("String(unconsumedBody.deref()===undefined)")
            .await
            .unwrap(),
        "true"
    );
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(30), server)
            .await
            .expect("Collecting the V8 stream must cancel the native body")
            .unwrap(),
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retirement_cancels_the_request_creation_realm_even_when_parent_holds_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).await.unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&bytes[..count]);
            if request.windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut bytes = [0; 1];
        socket.read(&mut bytes).await.unwrap()
    });
    let mut page = PageSession::open(
        "<!doctype html><body><iframe id=frame></iframe>",
        &format!("{origin}/"),
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    assert_eq!(page.eval_expression("(async()=>{const frame=document.getElementById('frame');globalThis.retainedResponse=await frame.contentWindow.fetch('/hold');frame.remove();return String(retainedResponse.status);})()").await.unwrap(),"200");
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(30), server)
            .await
            .expect("Retiring the request's actual realm must close a held response transport")
            .unwrap(),
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn timeout_abort_listener_survives_collection_until_delivery() {
    let mut page = PageSession::open("<body>", "https://timeout-lifetime.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_expression("globalThis.timeoutDelivered=false;(()=>{const signal=AbortSignal.timeout(20);signal.addEventListener('abort',()=>{timeoutDelivered=signal.aborted&&signal.reason.name==='TimeoutError';});})() ").await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("new Promise(resolve=>setTimeout(()=>resolve(timeoutDelivered),30))")
            .await
            .unwrap(),
        "true"
    );
}

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn pending_timeout_does_not_root_retired_document() {
    let mut page = PageSession::open("<body>", "https://timeout-lifetime.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_expression("(()=>{const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;const signal=child.AbortSignal.timeout(60000);signal.addEventListener('abort',()=>{});globalThis.timeoutRealmRefs=[new WeakRef(child),new WeakRef(signal)];frame.remove();return 'prepared';})()").await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("JSON.stringify(timeoutRealmRefs.map(ref=>ref.deref()===undefined))")
            .await
            .unwrap(),
        "[true,true]"
    );
}
