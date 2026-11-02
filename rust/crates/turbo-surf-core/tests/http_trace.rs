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
    streamed_report_error_and_unwritable_traces().await;
}

fn rows(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

// Serves `responses` in order, one per connection.
async fn serve(responses: Vec<&'static [u8]>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        for response in responses {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut bytes = [0; 4096];
                let n = s.read(&mut bytes).await.unwrap();
                head.extend_from_slice(&bytes[..n]);
            }
            s.write_all(response).await.unwrap();
        }
    });
    origin
}

// Runs inside the test above: the trace switch is process-global.
async fn streamed_report_error_and_unwritable_traces() {
    use turbo_surf_core::http_cache::HttpCache;
    use turbo_surf_core::net::{build_report_client, fetch_stream, send_report_request};
    let path = std::env::temp_dir().join(format!("turbo-http-trace2-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&path);
    std::env::set_var("TURBO_SURF_HTTP_TRACE", &path);
    let origin = serve(vec![
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 11\r\nConnection: close\r\n\r\n<p>done</p>",
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 20\r\nConnection: close\r\n\r\n<p>first",
        b"HTTP/1.1 204 No Content\r\nX-Report: seen\r\nConnection: close\r\n\r\n",
        b"HTTP/1.1 206 Partial Content\r\nETag: \"v1\"\r\nCache-Control: max-age=60\r\nContent-Range: bytes 0-1/4\r\nContent-Length: 2\r\nConnection: close\r\n\r\nab",
        b"HTTP/1.1 206 Partial Content\r\nETag: \"v1\"\r\nContent-Range: bytes 2-3/4\r\nContent-Length: 2\r\nConnection: close\r\n\r\ncd",
    ])
    .await;
    // A streamed body is traced once, complete or cancelled.
    let mut done = fetch_stream(&format!("{origin}/done"), FetchOptions::default())
        .await
        .unwrap();
    while done.body.next_chunk().await.unwrap().is_some() {}
    let mut cut = fetch_stream(&format!("{origin}/cut"), FetchOptions::default())
        .await
        .unwrap();
    assert_eq!(cut.body.next_chunk().await.unwrap().unwrap(), b"<p>first");
    cut.body.cancel();
    // Native reports trace their request and response.
    let response = send_report_request(
        &build_report_client(),
        "POST",
        &format!("{origin}/report"),
        &Default::default(),
        b"{}".to_vec(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(response.status, 204);
    // A completed range traces only the bytes fetched for the continuation.
    let cache = HttpCache::default();
    let options = |range: Option<&str>| {
        let mut options = FetchOptions {
            http_cache: Some(&cache),
            allow_non_html: true,
            ..Default::default()
        };
        if let Some(range) = range {
            options.headers.insert("range".into(), range.into());
        }
        options
    };
    let url = format!("{origin}/range");
    let mut part = fetch_stream(&url, options(Some("bytes=0-1")))
        .await
        .unwrap();
    while part.body.next_chunk().await.unwrap().is_some() {}
    let mut whole = fetch_stream(&url, options(None)).await.unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = whole.body.next_chunk().await.unwrap() {
        bytes.extend(chunk);
    }
    assert_eq!(bytes, b"abcd");
    drop(whole);
    // A refused connection is traced as an error event.
    assert!(
        fetch_html("http://127.0.0.1:1/refused", FetchOptions::default())
            .await
            .is_err()
    );

    let rows = rows(&path);
    let bodies: Vec<_> = rows.iter().filter(|r| r["event"] == "body").collect();
    assert_eq!(bodies[0]["body"], "<p>done</p>");
    assert_eq!(bodies[0]["streamed"], true);
    assert_eq!(bodies[0]["incomplete"], false);
    assert_eq!(bodies[1]["body"], "<p>first");
    assert_eq!(bodies[1]["incomplete"], true);
    let report: Vec<_> = rows
        .iter()
        .filter(|r| r["url"] == format!("{origin}/report"))
        .collect();
    assert_eq!(report[0]["destination"], "report");
    assert_eq!(report[0]["body_bytes"], serde_json::json!([123, 125]));
    assert_eq!(report[1]["status"], 204);
    assert_eq!(report[0]["id"], report[1]["id"]);
    let range = bodies.last().unwrap();
    assert_eq!(range["url"], url);
    assert_eq!(range["body"], "cd");
    assert_eq!(range["cache_prefix_bytes"], 2);
    let error = rows.last().unwrap();
    assert_eq!(error["event"], "error");
    assert_eq!(error["url"], "http://127.0.0.1:1/refused");
    assert!(!error["error"].as_str().unwrap().is_empty());
    std::fs::remove_file(&path).unwrap();

    // An unwritable trace target is reported on stderr and never fails the fetch.
    let dir = std::env::temp_dir().join(format!("turbo-http-trace-dir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("TURBO_SURF_HTTP_TRACE", &dir);
    let origin = serve(vec![
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
    ])
    .await;
    let page = fetch_html(&format!("{origin}/"), FetchOptions::default()).await;
    std::env::remove_var("TURBO_SURF_HTTP_TRACE");
    assert_eq!(page.unwrap().html, "ok");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir(&dir).unwrap();
}
