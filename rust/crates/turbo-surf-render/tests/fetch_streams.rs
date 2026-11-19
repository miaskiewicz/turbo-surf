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

const RETIRED_TIMEOUT_SETUP: &str = r#"(()=>{
    globalThis.timeoutFrameLoaded=false;
    const frame=document.createElement('iframe');
    frame.addEventListener('load',()=>{timeoutFrameLoaded=true;});
    document.body.append(frame);
    const child=frame.contentWindow;
    const signal=child.AbortSignal.timeout(60000);
    signal.addEventListener('abort',()=>{});
    globalThis.timeoutRealmRefs=[new WeakRef(child),new WeakRef(signal)];
    if(timeoutFrameLoaded)throw new Error('frame must still be initializing at retirement');
    frame.remove();
    return 'prepared';
})()"#;

fn retired_timeout_load_state(page: &mut PageSession) -> serde_json::Value {
    serde_json::from_str(&page.read_engine(
        "return {resources:__tsNs.pendingResourceCount(),fetches:Number(__tsNs.pendingFetchCount()),loaded:timeoutFrameLoaded};"
    ).unwrap()).unwrap()
}

async fn settle_retired_timeout_load(page: &mut PageSession) {
    assert_eq!(
        page.eval_engine_expression("__tsNs.waitForDocumentResources().then(()=>'settled')")
            .await
            .unwrap(),
        "settled"
    );
    assert_eq!(
        retired_timeout_load_state(page),
        serde_json::json!({"resources":0,"fetches":0,"loaded":false})
    );
}

struct RetiredTimeoutFailureDiagnostics {
    frames_before_gc: serde_json::Value,
    contexts_before_gc: serde_json::Value,
    loads_before_settlement: serde_json::Value,
    loads_after_settlement: serde_json::Value,
}

impl Drop for RetiredTimeoutFailureDiagnostics {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "retired-timeout pre-gc native frames={} contexts={} loads_before={} loads_after={}",
                self.frames_before_gc, self.contexts_before_gc,
                self.loads_before_settlement, self.loads_after_settlement,
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn pending_timeout_does_not_root_retired_document() {
    let mut page = PageSession::open("<body>", "https://timeout-lifetime.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression(RETIRED_TIMEOUT_SETUP).await.unwrap(),
        "prepared"
    );
    let loads_before_settlement = retired_timeout_load_state(&mut page);
    settle_retired_timeout_load(&mut page).await;
    // Observe registry state without another guest turn or WeakRef read.
    let _failure_diagnostics = RetiredTimeoutFailureDiagnostics {
        frames_before_gc: page.frame_lifecycle_diagnostics_for_testing(),
        contexts_before_gc: page.context_registry_diagnostics_for_testing(),
        loads_before_settlement,
        loads_after_settlement: retired_timeout_load_state(&mut page),
    };
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

#[tokio::test(flavor = "current_thread")]
async fn retired_timeout_collection_waits_for_actual_initial_frame_load_unwind() {
    let mut page = PageSession::open("<body>", "https://timeout-lifetime.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(
        page.read_engine(&format!("return {RETIRED_TIMEOUT_SETUP};"))
            .unwrap(),
        "\"prepared\""
    );
    let pending = retired_timeout_load_state(&mut page);
    assert!(pending["resources"].as_u64().unwrap() > 0, "{pending}");
    assert!(pending["fetches"].as_u64().unwrap() > 0, "{pending}");
    assert_eq!(pending["loaded"], false);
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.read_engine("return timeoutRealmRefs.map(ref=>ref.deref()===undefined);")
            .unwrap(),
        "[false,false]",
        "the genuine suspended frame-load continuation still owns its retired realm"
    );
    settle_retired_timeout_load(&mut page).await;
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

#[tokio::test(flavor = "current_thread")]
async fn earlier_native_timeout_precedes_a_later_window_timer() {
    let mut page = PageSession::open("<body>", "https://timeout-clock.test/", "", "", 5000)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let actual = page.eval_expression(r#"(async()=>{const signal=AbortSignal.timeout(1000);let delivered=false;signal.addEventListener('abort',()=>{delivered=signal.aborted&&signal.reason.name==='TimeoutError';});await new Promise(resolve=>setTimeout(resolve,2000));return JSON.stringify({delivered,aborted:signal.aborted,reason:signal.aborted?signal.reason.name:null});})()"#).await.unwrap();
    let elapsed = started.elapsed();
    eprintln!("timeout-clock-native elapsed={elapsed:?} actual={actual}");
    assert_eq!(
        actual,
        r#"{"delivered":true,"aborted":true,"reason":"TimeoutError"}"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn peer_native_timeout_does_not_postpone_root_http_completion() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&bytes[..count]);
        }
        assert!(String::from_utf8_lossy(&request).starts_with("GET /root "));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nroot")
            .await
            .unwrap();
    });
    let mut page = PageSession::open("<body>", &origin, "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval_expression("(()=>{const frame=document.createElement('iframe');document.body.append(frame);globalThis.peerTimeout=frame.contentWindow.AbortSignal.timeout(1000);return String(peerTimeout.aborted);})()").await.unwrap(), "false");
    assert_eq!(page.eval_expression("(async()=>{const response=await fetch('/root');return JSON.stringify([await response.text(),peerTimeout.aborted]);})()").await.unwrap(), r#"["root",false]"#);
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_timeout_keeps_its_function_relevant_global() {
    let mut page = PageSession::open("<body>", "https://borrowed-timeout.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval_expression("(async()=>{const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;const signal=child.AbortSignal.timeout.call(AbortSignal,20);const brands=[signal instanceof child.AbortSignal,signal instanceof AbortSignal];await new Promise(resolve=>signal.addEventListener('abort',resolve,{once:true}));return JSON.stringify([...brands,signal.aborted,signal.reason.name,signal.reason instanceof child.DOMException,signal.reason instanceof DOMException]);})()").await.unwrap(), r#"[true,false,true,"TimeoutError",true,false]"#);
}
