use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::http_cache::{CacheMode, HttpCache};
use turbo_surf_core::net::{fetch_stream, FetchOptions};

#[tokio::test]
async fn cache_is_partitioned_policy_checked_and_committed_only_after_complete_body() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/cached", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let requests = requests.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let n = socket.read(&mut bytes).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    head.extend_from_slice(&bytes[..n]);
                    if head.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.fetch_add(1, Ordering::SeqCst);
                socket.write_all(b"HTTP/1.1 200 Authored\r\nCache-Control: max-age=3600\r\nContent-Type: text/plain\r\nContent-Length: 4\r\nConnection: close\r\n\r\n\x00\xffAB").await.unwrap();
            });
        }
    });
    let cache = HttpCache::default();
    let options = |partition: &str, cache_mode| FetchOptions {
        http_cache: Some(&cache),
        cache_mode,
        cache_partition: partition.into(),
        allow_non_html: true,
        ..Default::default()
    };
    let mut first = fetch_stream(&url, options("site-a", CacheMode::Default))
        .await
        .unwrap();
    first.body.cancel();
    assert!(
        fetch_stream(&url, options("site-a", CacheMode::OnlyIfCached))
            .await
            .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let mut complete = fetch_stream(&url, options("site-a", CacheMode::Default))
        .await
        .unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = complete.body.next_chunk().await.unwrap() {
        bytes.extend(chunk);
    }
    assert_eq!(bytes, [0, 255, 65, 66]);
    let mut cached = fetch_stream(&url, options("site-a", CacheMode::OnlyIfCached))
        .await
        .unwrap();
    assert_eq!(cached.status_text, "Authored");
    assert_eq!(cached.body.next_chunk().await.unwrap().unwrap(), bytes);
    assert!(
        fetch_stream(&url, options("site-b", CacheMode::OnlyIfCached))
            .await
            .is_err()
    );
    let deny = |_: &str, _: bool| false;
    let mut blocked = options("site-a", CacheMode::OnlyIfCached);
    blocked.request_policy = Some(&deny);
    assert!(fetch_stream(&url, blocked).await.is_err());
    let deny_response = |_: &turbo_surf_core::request_context::RequestContext,
                         _: &str,
                         _: &[(String, String)]| false;
    let mut blocked = options("site-a", CacheMode::OnlyIfCached);
    blocked.response_policy = Some(&deny_response);
    assert!(fetch_stream(&url, blocked).await.is_err());
    let mut capped = options("site-a", CacheMode::OnlyIfCached);
    capped.max_bytes = Some(3);
    assert!(fetch_stream(&url, capped)
        .await
        .unwrap()
        .body
        .next_chunk()
        .await
        .is_err());
    let mut unsafe_request = options("site-a", CacheMode::Default);
    unsafe_request.method = Some("POST".into());
    let mut response = fetch_stream(&url, unsafe_request).await.unwrap();
    while response.body.next_chunk().await.unwrap().is_some() {}
    assert!(
        fetch_stream(&url, options("site-a", CacheMode::OnlyIfCached))
            .await
            .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 3);
    server.abort();
}

#[tokio::test]
async fn redirect_fragments_are_inherited_replaced_and_cleared_with_cached_hops() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let received = requests.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let received = received.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let n = socket.read(&mut bytes).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    head.extend_from_slice(&bytes[..n]);
                    if head.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                received.fetch_add(1, Ordering::SeqCst);
                let text = String::from_utf8_lossy(&head);
                let path = text.split_whitespace().nth(1).unwrap();
                let response=match path{
                    "/inherit"=>"HTTP/1.1 301 Moved Permanently\r\nLocation: /target\r\nCache-Control: max-age=3600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    "/replace"=>"HTTP/1.1 302 Found\r\nLocation: /target#new\r\nCache-Control: max-age=3600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    "/clear"=>"HTTP/1.1 302 Found\r\nLocation: /target#\r\nCache-Control: max-age=3600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    _=>"HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                };
                socket.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    let cache = HttpCache::default();
    let targets = std::sync::Mutex::new(Vec::<String>::new());
    let policy = |target: &str, _: bool| {
        targets.lock().unwrap().push(target.into());
        true
    };
    for (path, fragment) in [
        ("inherit", "first"),
        ("inherit", "second"),
        ("replace", "old"),
        ("clear", "old"),
    ] {
        targets.lock().unwrap().clear();
        let mut response = fetch_stream(
            &format!("{origin}/{path}#{fragment}"),
            FetchOptions {
                http_cache: Some(&cache),
                cache_partition: "fragment-control".into(),
                request_policy: Some(&policy),
                allow_non_html: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        while response.body.next_chunk().await.unwrap().is_some() {}
        assert_eq!(response.final_url, format!("{origin}/target"));
        let expected = match path {
            "replace" => format!("{origin}/target#new"),
            "clear" => format!("{origin}/target#"),
            _ => format!("{origin}/target#{fragment}"),
        };
        assert_eq!(targets.lock().unwrap()[1], expected);
    }
    assert_eq!(requests.load(Ordering::SeqCst), 4);
    server.abort();
}

#[tokio::test]
async fn network_effect_observer_uses_raw_headers_before_policy_and_never_replays_cached_responses()
{
    use turbo_surf_core::request_context::{Credentials, Destination, RequestContext, RequestMode};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut head = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let n = socket.read(&mut bytes).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    head.extend_from_slice(&bytes[..n]);
                    if head.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&head);
                let response = if text
                    .to_ascii_lowercase()
                    .contains("if-none-match: \"stable\"")
                {
                    "HTTP/1.1 304 Not Modified\r\nCache-Control: max-age=3600\r\nETag: \"stable\"\r\nX-Raw: validation\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nETag: \"stable\"\r\nX-Raw: initial\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                };
                socket.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    let cache = HttpCache::default();
    let observed = std::sync::Mutex::new(Vec::<Vec<(String, String)>>::new());
    let observer = |_: &RequestContext, url: &str, headers: &[(String, String)]| {
        assert_eq!(url, format!("{origin}/effect"));
        observed.lock().unwrap().push(headers.to_vec());
    };
    let context = RequestContext::resource(&origin, Destination::Empty, RequestMode::SameOrigin);
    let options = |mode| FetchOptions {
        http_cache: Some(&cache),
        cache_mode: mode,
        cache_partition: "effects".into(),
        response_side_effects: Some(&observer),
        context: Some(context.clone()),
        allow_non_html: true,
        ..Default::default()
    };
    let url = format!("{origin}/effect");
    for mode in [
        CacheMode::Default,
        CacheMode::Default,
        CacheMode::NoCache,
        CacheMode::Default,
    ] {
        let mut response = fetch_stream(&url, options(mode)).await.unwrap();
        while response.body.next_chunk().await.unwrap().is_some() {}
    }
    assert_eq!(observed.lock().unwrap().len(), 2);
    {
        let records = observed.lock().unwrap();
        assert!(records[0]
            .iter()
            .any(|(name, value)| name == "x-raw" && value == "initial"));
        assert!(records[1]
            .iter()
            .any(|(name, value)| name == "x-raw" && value == "validation"));
        assert!(!records[1].iter().any(|(name, _)| name == "content-type"));
    }
    let mut omitted = options(CacheMode::Reload);
    omitted.context.as_mut().unwrap().credentials = Credentials::Omit;
    let mut response = fetch_stream(&url, omitted).await.unwrap();
    while response.body.next_chunk().await.unwrap().is_some() {}
    let deny = |_: &RequestContext, _: &str, _: &[(String, String)]| false;
    let mut denied = options(CacheMode::Reload);
    denied.response_policy = Some(&deny);
    assert!(fetch_stream(&url, denied).await.is_err());
    assert_eq!(observed.lock().unwrap().len(), 3);
    server.abort();
}
