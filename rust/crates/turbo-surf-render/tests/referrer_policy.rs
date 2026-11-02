use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;
struct Server {
    origin: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(cross: String) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let cross = cross.clone();
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
                    if bytes.len() > 16384 {
                        return;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap();
                let referrer = request
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("referer"))
                    .map(|(_, value)| value.trim());
                let (status, extra, body) =
                    if let Some(policy) = path.strip_prefix("/redirect?policy=") {
                        (
                            "302 Found",
                            format!("Location: {cross}/echo\r\nReferrer-Policy: {policy}\r\n"),
                            String::new(),
                        )
                    } else {
                        (
                            "200 OK",
                            String::new(),
                            json!({"referer":referrer}).to_string(),
                        )
                    };
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    Server { origin, task }
}
#[tokio::test(flavor = "current_thread")]
async fn native_document_request_and_redirect_referrer_policy_matches_chrome_headers() {
    let cross = server(String::new()).await;
    let source = server(cross.origin.clone()).await;
    let html = format!(
        "<!doctype html><body data-cross='{}'>Authored referrer policy control</body>",
        cross.origin
    );
    let url = format!("{}/page?authored=value#fragment", source.origin);
    let mut page = PageSession::open_with_response_headers(
        &html,
        &url,
        "",
        "",
        60_000,
        "",
        Default::default(),
        &[("Referrer-Policy".into(), "same-origin".into())],
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/referrer-policy-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page
        .eval(&format!(
            "(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})()"
        ))
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/referrer-policy-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, fixture["result"]);
}
