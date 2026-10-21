use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn late_realms_do_not_receive_earlier_response_clear_events() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut bytes = [0; 8192];
                let size = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..size]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap();
                let script = path.starts_with("/clear-register");
                let body = if script {
                    "globalThis.registerClearPeer();"
                } else {
                    "clear"
                };
                let content_type = if script {
                    "text/javascript"
                } else {
                    "text/html"
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nClear-Site-Data: \"storage\"\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let mut page = PageSession::open(
        "<!doctype html><body>",
        &format!("http://127.0.0.1:{port}/"),
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/storage-eligibility-probe.js")
        .trim()
        .trim_end_matches(';');
    let result = page.eval_json_expression(probe).await;
    server.abort();
    let actual: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/storage-eligibility-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}
