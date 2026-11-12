//! Host transport observation stays usable while an isolate hydrates or awaits its body.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::{NetworkEventLog, PageSession, PageSessionOptions};

#[tokio::test(flavor = "current_thread")]
async fn opening_headers_and_body_completion_are_independent_host_records() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let (release, body_gate) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut chunk = [0; 1024];
            let length = socket.read(&mut chunk).await.unwrap();
            assert!(
                length > 0,
                "client closed before completing its request headers"
            );
            request.extend_from_slice(&chunk[..length]);
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n").await.unwrap();
        let _ = body_gate.await;
        socket.write_all(b"{\"ok\":true}").await.unwrap();
    });
    let log = NetworkEventLog::default();
    let mut opening = Box::pin(PageSession::open_with_document_response_and_event_log(
        "<script>fetch('/slow').then(r=>r.json()).then(value=>window.done=value.ok)</script>",
        &base,
        "",
        "",
        60_000,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        PageSessionOptions::default(),
        Some(log.clone()),
    ));
    let page = tokio::select! {
        _=log.wait_for_events(0)=>None,
        result=&mut opening=>Some(result.unwrap()),
    };
    let headers: serde_json::Value = serde_json::from_str(&log.drain_batch()).unwrap();
    let record = &headers["entries"][0];
    assert_eq!(record["event"], "response");
    assert_eq!(record["status"], 200);
    assert_eq!(record["bodyPending"], true);
    let id = record["id"].clone();
    log.acknowledge(headers["revision"].as_str().unwrap().parse().unwrap());
    release.send(()).unwrap();
    let mut page = match page {
        Some(page) => page,
        None => opening.await.unwrap(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), log.wait_for_events(0))
        .await
        .unwrap();
    let completed: serde_json::Value = serde_json::from_str(&log.drain_batch()).unwrap();
    let body = completed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["event"] == "responsebody" && row["id"] == id)
        .unwrap();
    assert_eq!(body["body"], "{\"ok\":true}");
    assert_eq!(body["bodyIncomplete"], false);
    assert_eq!(
        page.eval_expression("String(window.done)").await.unwrap(),
        "true"
    );
    page.close();
    assert!(log.is_closed());
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn an_external_host_response_keeps_a_collected_no_store_body_until_completion() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let (release, gate) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut chunk = [0; 1024];
            let length = socket.read(&mut chunk).await.unwrap();
            assert!(length > 0);
            request.extend_from_slice(&chunk[..length]);
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: 4\r\nConnection: close\r\n\r\n").await.unwrap();
        gate.await.unwrap();
        socket.write_all(b"body").await.unwrap();
    });
    let log = NetworkEventLog::default();
    let mut page = PageSession::open_with_document_response_and_event_log(
        "<!doctype html><body>",
        &base,
        "",
        "",
        60_000,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        PageSessionOptions::default(),
        Some(log.clone()),
    )
    .await
    .unwrap();
    assert_eq!(page.eval_expression("(async()=>{const response=await fetch('/hold');globalThis.unconsumedBody=new WeakRef(response.body);return 'headers';})()").await.unwrap(), "headers");
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("String(unconsumedBody.deref()===undefined)")
            .await
            .unwrap(),
        "true"
    );
    let headers: serde_json::Value = serde_json::from_str(&log.drain_batch()).unwrap();
    assert_eq!(headers["entries"][0]["event"], "response");
    log.acknowledge(headers["revision"].as_str().unwrap().parse().unwrap());
    release.send(()).unwrap();
    server.await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), log.wait_for_events(0))
        .await
        .unwrap();
    let completed: serde_json::Value = serde_json::from_str(&log.drain_batch()).unwrap();
    let body = completed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["event"] == "responsebody")
        .unwrap();
    assert_eq!(body["body"], "body");
    assert_eq!(body["bodyIncomplete"], false);
    page.close();
}
