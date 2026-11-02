//! Page request identity: a session's configured User-Agent is the `User-Agent` of
//! fetches (guest headers cannot override it) and image loads, and `mode: "same-origin"` to
//! another origin rejects with a network TypeError before any request, like Chrome.
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

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
                seen.lock().unwrap().push(format!(
                    "{} | {agent}",
                    request.lines().next().unwrap_or("")
                ));
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{agent}", agent.len());
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    origin
}

#[tokio::test(flavor = "current_thread")]
async fn configured_user_agent_and_same_origin_mode_shape_page_fetches() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let origin = server(seen.clone()).await;
    let mut page = PageSession::open(
        "<body>",
        &format!("{origin}/page"),
        "",
        "SurfTest/1.0 (identity)",
        5000,
    )
    .await
    .unwrap();
    let agent = page
        .eval_expression(
            "fetch('/ua',{headers:{'User-Agent':'guest/9'}}).then(response=>response.text())",
        )
        .await
        .unwrap();
    assert_eq!(agent, "SurfTest/1.0 (identity)");
    let cross = origin.replace("127.0.0.1", "localhost");
    let rejected = page
        .eval_expression(&format!(
            "fetch('{cross}/never',{{mode:'same-origin'}}).then(()=>'resolved',error=>error.name)"
        ))
        .await
        .unwrap();
    assert_eq!(rejected, "TypeError");
    // Image loads carry the configured identity too.
    let image = page
        .eval_expression(
            "new Promise(resolve=>{const image=new Image();image.onload=()=>resolve('load');image.onerror=()=>resolve('error');image.src='/pixel';})",
        )
        .await
        .unwrap();
    assert_eq!(image, "error");
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "GET /ua HTTP/1.1 | SurfTest/1.0 (identity)",
            "GET /pixel HTTP/1.1 | SurfTest/1.0 (identity)"
        ]
    );
}
