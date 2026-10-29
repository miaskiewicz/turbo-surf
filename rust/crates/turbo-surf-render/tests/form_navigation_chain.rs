//! Native form navigation over a localhost server: `accept-charset="utf-8"` encodes
//! with UTF-8 in a windows-1252 document (as Chrome 156 does), the request carries the
//! session's configured User-Agent, and documents that keep auto-submitting stop at the
//! native 20-navigation budget with a named error.
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

const LOOP_PAGE: &str =
    "<form action='/loop'><input name='n' value='1'></form><script>document.forms[0].submit()</script>";

async fn server(seen: Arc<Mutex<Vec<String>>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut chunk = [0; 1024];
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(count) => request.extend_from_slice(&chunk[..count]),
                    }
                }
                let request = String::from_utf8_lossy(&request).to_string();
                let agent = request
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
                    .map_or("", |(_, value)| value.trim())
                    .to_string();
                let line = request.lines().next().unwrap_or("").to_string();
                let body = if line.starts_with("GET /loop") {
                    LOOP_PAGE
                } else {
                    "<p>done</p>"
                };
                seen.lock().unwrap().push(format!("{line} | {agent}"));
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    origin
}

async fn open(origin: &str, html: &str) -> PageSession {
    PageSession::open_with_response_headers(
        html,
        &format!("{origin}/start"),
        "",
        "SurfForms/2.0",
        10_000,
        "",
        ClientHints::default(),
        &[(
            "Content-Type".to_string(),
            "text/html; charset=windows-1252".to_string(),
        )],
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn accept_charset_utf8_and_user_agent_shape_the_submission() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let origin = server(seen.clone()).await;
    let mut page = open(
        &origin,
        "<form action='/done' accept-charset='UTF-8'><input name='q' value='\u{e9}'></form>",
    )
    .await;
    page.eval("document.forms[0].submit()").await.unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        ["GET /done?q=%C3%A9 HTTP/1.1 | SurfForms/2.0"]
    );
    assert_eq!(
        page.eval_expression("document.querySelector('p').textContent")
            .await
            .unwrap(),
        "done"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn auto_submitting_documents_stop_at_the_navigation_budget() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let origin = server(seen.clone()).await;
    let mut page = open(&origin, "<body>").await;
    let error = page
        .eval(
            "document.body.innerHTML=\"<form action='/loop'></form>\"; document.forms[0].submit()",
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "Native form navigation exceeded its redirect/document budget"
    );
    assert_eq!(seen.lock().unwrap().len(), 20);
}
