use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

async fn compare_body_probe(probe: &str, fixture: &str) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(AtomicUsize::new(0));
    let received = requests.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let received = received.clone();
            tokio::spawn(async move {
                let exchange = async move {
                    let mut bytes = Vec::new();
                    let mut chunk = [0; 2048];
                    let head_end = loop {
                        if let Some(at) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                            break at + 4;
                        }
                        assert!(bytes.len() < 64 * 1024, "fixture header limit");
                        let size = socket.read(&mut chunk).await.unwrap();
                        assert_ne!(size, 0, "incomplete fixture headers");
                        bytes.extend_from_slice(&chunk[..size]);
                    };
                    let length = String::from_utf8_lossy(&bytes[..head_end])
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map_or(0, |(_, value)| value.trim().parse::<usize>().unwrap());
                    assert!(length <= 64 * 1024, "fixture body limit");
                    while bytes.len() < head_end + length {
                        let size = socket.read(&mut chunk).await.unwrap();
                        assert_ne!(size, 0, "incomplete fixture body");
                        bytes.extend_from_slice(&chunk[..size]);
                    }
                    received.fetch_add(1, Ordering::Relaxed);
                    let body = &bytes[head_end..head_end + length];
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                    socket.write_all(body).await.unwrap();
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), exchange)
                    .await
                    .unwrap();
            });
        }
    });
    let mut page = PageSession::open(
        "<!doctype html><body>",
        &format!("http://127.0.0.1:{port}/"),
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let probe = probe.trim().trim_end_matches(';');
    let result = page.eval_json_expression(probe).await;
    server.abort();
    let actual: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(fixture).unwrap();
    assert_eq!(actual, control["result"]);
    let expected_requests = control["wire"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|request| request["path"] == "/echo")
        .count();
    assert_eq!(requests.load(Ordering::Relaxed), expected_requests);
}

#[tokio::test(flavor = "current_thread")]
async fn request_body_construction_and_consumption_match_source_pinned_chrome() {
    compare_body_probe(
        include_str!("../../../../harness/detection/request-body-probe.js"),
        include_str!("../../../../harness/detection/fixtures/request-body-chrome155.json"),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn adversarial_body_brands_and_transfer_match_source_pinned_chrome() {
    compare_body_probe(
        include_str!("../../../../harness/detection/request-body-details-probe.js"),
        include_str!("../../../../harness/detection/fixtures/request-body-details-chrome155.json"),
    )
    .await;
}
