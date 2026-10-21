use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::http_cache::{CacheMode, HttpCache};
use turbo_surf_core::net::{fetch_stream, ErrorCode, FetchOptions};

#[tokio::test]
async fn background_cache_preserves_binary_bytes_caps_and_owner_cancellation() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (closed, mut closures) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let closed = closed.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let count = socket.read(&mut bytes).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&bytes[..count]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let hold =
                    String::from_utf8_lossy(&request).split_whitespace().nth(1) == Some("/owner");
                socket.write_all(b"HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nContent-Length: 5\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n").await.unwrap();
                if hold {
                    let mut byte = [0; 1];
                    closed.send(socket.read(&mut byte).await.unwrap()).unwrap();
                } else {
                    socket.write_all(&[0, 255, 128, 65, 0]).await.unwrap();
                }
            });
        }
    });
    let cache = HttpCache::default();
    let options = |mode, max_bytes| FetchOptions {
        http_cache: Some(&cache),
        cache_mode: mode,
        cache_partition: "background".into(),
        max_bytes: Some(max_bytes),
        allow_non_html: true,
        ..Default::default()
    };
    let (owner, watch) = tokio::sync::watch::channel(false);
    let mut body = fetch_stream(&format!("{origin}/finite"), options(CacheMode::Default, 32))
        .await
        .unwrap()
        .body
        .background_cache(watch);
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !body.background_complete() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut cached = fetch_stream(
        &format!("{origin}/finite"),
        options(CacheMode::OnlyIfCached, 32),
    )
    .await
    .unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = cached.body.next_chunk().await.unwrap() {
        bytes.extend(chunk);
    }
    assert_eq!(bytes, [0, 255, 128, 65, 0]);
    let mut queued = Vec::new();
    while let Some(chunk) = body.next_chunk().await.unwrap() {
        queued.extend(chunk);
    }
    assert_eq!(queued, bytes);
    drop(owner);
    let (_owner, watch) = tokio::sync::watch::channel(false);
    let mut body = fetch_stream(&format!("{origin}/cap"), options(CacheMode::Default, 4))
        .await
        .unwrap()
        .body
        .background_cache(watch);
    let mut received = 0;
    loop {
        match body.next_chunk().await {
            Ok(Some(bytes)) => received += bytes.len(),
            Err(error) => {
                assert_eq!(error.code, ErrorCode::BodyTooLarge);
                break;
            }
            Ok(None) => panic!("background fill must retain the body cap"),
        }
    }
    assert!(received <= 4);
    assert_eq!(body.background_status(), Some(false));
    assert!(fetch_stream(
        &format!("{origin}/cap"),
        options(CacheMode::OnlyIfCached, 32)
    )
    .await
    .is_err());
    let (owner, watch) = tokio::sync::watch::channel(false);
    let mut body = fetch_stream(&format!("{origin}/owner"), options(CacheMode::Default, 32))
        .await
        .unwrap()
        .body
        .background_cache(watch);
    assert!(body.detach_cache_consumer());
    // Closing the external owner sender must wake a task waiting on an infinite
    // transport even though no guest or consumer remains to send an abort.
    drop(owner);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), closures.recv())
            .await
            .unwrap(),
        Some(0)
    );
    assert!(fetch_stream(
        &format!("{origin}/owner"),
        options(CacheMode::OnlyIfCached, 32)
    )
    .await
    .is_err());
    server.abort();
}
