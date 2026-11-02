use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use turbo_surf_render::PageSession;

fn chrome_result(name: &str) -> String {
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/window-timer-clock-chrome155.json"
    ))
    .unwrap();
    match &control["result"][name] {
        serde_json::Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

async fn read_request_headers(socket: &mut TcpStream) {
    let mut headers = Vec::new();
    let mut chunk = [0; 1024];
    while !headers.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let count = socket.read(&mut chunk).await.unwrap();
        assert!(count > 0, "request closed before complete HTTP headers");
        headers.extend_from_slice(&chunk[..count]);
        assert!(
            headers.len() <= 16 * 1024,
            "fixture request headers exceed bound"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_body_completion_precedes_a_later_window_timer() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request_headers(&mut socket).await;
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 4\r\nConnection: close\r\n\r\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        socket.write_all(b"done").await.unwrap();
    });
    let mut page = PageSession::open("<body>", &origin, "", "", 60_000)
        .await
        .unwrap();
    assert_eq!(page.eval_expression("(async()=>{const response=await fetch('/body');let completed=false;response.text().then(()=>completed=true);return new Promise(resolve=>setTimeout(()=>resolve(completed),100));})()").await.unwrap(), chrome_result("bodyBeforeLaterTimer"));
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_window_deadline_still_cancels_stalled_native_io() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (accepted, acceptance) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request_headers(&mut socket).await;
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 4\r\nConnection: close\r\n\r\n").await.unwrap();
        let _ = accepted.send(());
        // The body never arrives; only the Window deadline can end this fetch.
        let mut byte = [0];
        socket.read(&mut byte).await.unwrap()
    });
    let mut page = PageSession::open("<body>", &origin, "", "", 60_000)
        .await
        .unwrap();
    assert_eq!(page.eval_expression("(async()=>{const controller=new AbortController();const response=await fetch('/stall',{signal:controller.signal});setTimeout(()=>controller.abort(),30);try{await response.text();return 'unexpected success';}catch(error){return error.name;}})()").await.unwrap(), chrome_result("windowDeadlineAbort"));
    acceptance.await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(30), server)
            .await
            .unwrap()
            .unwrap(),
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn held_native_timeout_precedes_window_timer_without_public_listeners() {
    let mut page = PageSession::open("<body>", "https://timeout-clock.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval_expression("(async()=>{const signal=AbortSignal.timeout(20);return new Promise(resolve=>setTimeout(()=>resolve(signal.aborted),30));})()").await.unwrap(), chrome_result("heldNativeTimeout"));
}
