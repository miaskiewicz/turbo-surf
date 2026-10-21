//! Trace an actual localhost POST without external network dependencies.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::net::{fetch_html, FetchOptions};

#[tokio::test(flavor = "current_thread")]
async fn raw_trace_keeps_post_payload_response_cookie_and_decoded_body() {
    let path = std::env::temp_dir().join(format!("turbo-http-trace-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&path);
    std::env::set_var("TURBO_SURF_HTTP_TRACE", &path);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut data = vec![0; 8192];
        let n = s.read(&mut data).await.unwrap();
        let req = String::from_utf8_lossy(&data[..n]);
        assert!(req.starts_with("POST /"));
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nSet-Cookie: trace_test=x; HttpOnly\r\nContent-Length: 11\r\nConnection: close\r\n\r\n<p>done</p>").await.unwrap();
    });
    let r = fetch_html(
        &url,
        FetchOptions {
            method: Some("POST".into()),
            body: Some("token=a%26b".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    server.await.unwrap();
    std::env::remove_var("TURBO_SURF_HTTP_TRACE");
    assert_eq!(r.html, "<p>done</p>");
    let rows: Vec<serde_json::Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["event"], "request");
    assert_eq!(rows[0]["body"], "token=a%26b");
    assert_eq!(rows[1]["status"], 200);
    assert!(rows[1]["headers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h[0] == "set-cookie"));
    assert_eq!(rows[2]["body"], "<p>done</p>");
    assert_eq!(rows[0]["id"], rows[1]["id"]);
    assert_eq!(rows[1]["id"], rows[2]["id"]);
    assert!(rows[0]["time_us"].is_number());
    std::fs::remove_file(path).unwrap();
}
