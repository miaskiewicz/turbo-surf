//! HTTP cache edge paths against a scripted localhost origin: partial-content
//! continuation (RFC 9110 14.4/13.1.5 If-Range), 304 revalidation of HEAD, and
//! Fetch's "default" mode turning into "no-store" for author conditional headers.
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::http_cache::{CacheMode, HttpCache};
use turbo_surf_core::net::{fetch_stream, FetchOptions, StreamingResult};
use turbo_surf_core::request_context::RequestContext;

type Script = fn(&str, &str) -> &'static str;

// Answers each request with `script(path, lowercased head)` and records the heads.
async fn origin(script: Script) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                while !head.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut bytes = [0; 1024];
                    let n = socket.read(&mut bytes).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    head.extend_from_slice(&bytes[..n]);
                }
                let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                log.lock().unwrap().push(head.clone());
                let _ = socket.write_all(script(&path, &head).as_bytes()).await;
            });
        }
    });
    (origin, seen)
}

fn options<'a>(cache: &'a HttpCache, mode: CacheMode) -> FetchOptions<'a> {
    FetchOptions {
        http_cache: Some(cache),
        cache_mode: mode,
        cache_partition: "edges".into(),
        allow_non_html: true,
        ..Default::default()
    }
}

fn ranged<'a>(cache: &'a HttpCache, range: &str) -> FetchOptions<'a> {
    let mut options = options(cache, CacheMode::Default);
    options.headers.insert("range".into(), range.into());
    options
}

async fn body(mut response: StreamingResult) -> Vec<u8> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.body.next_chunk().await.unwrap() {
        bytes.extend(chunk);
    }
    bytes
}

const FIRST_HALF: &str = "HTTP/1.1 206 Partial Content\r\nETag: \"v1\"\r\nCache-Control: max-age=3600\r\nContent-Range: bytes 0-1/4\r\nContent-Length: 2\r\nConnection: close\r\n\r\nab";

fn partial_script(path: &str, head: &str) -> &'static str {
    if !head.contains("if-range") {
        return FIRST_HALF;
    }
    match path {
        // The validator changed between the parts: a 206 for another representation.
        "/changed" => "HTTP/1.1 206 Partial Content\r\nETag: \"v2\"\r\nContent-Range: bytes 2-3/4\r\nContent-Length: 2\r\nConnection: close\r\n\r\ncd",
        // The origin answers the continuation with 304 (no body to complete from).
        "/not-modified" => "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
        _ => "HTTP/1.1 206 Partial Content\r\nETag: \"v1\"\r\nContent-Range: bytes 2-3/4\r\nContent-Length: 2\r\nConnection: close\r\n\r\ncd",
    }
}

#[tokio::test]
async fn range_continuations_are_validated_before_they_complete_a_stored_part() {
    let (origin, seen) = origin(partial_script).await;
    let cache = HttpCache::default();
    // The first part is stored sparse; the continuation asks only for bytes 2-3
    // under If-Range and is stitched behind the stored prefix.
    let first = fetch_stream(&format!("{origin}/ok"), ranged(&cache, "bytes=0-1"))
        .await
        .unwrap();
    assert_eq!((first.status, body(first).await), (206, b"ab".to_vec()));
    let whole = fetch_stream(&format!("{origin}/ok"), options(&cache, CacheMode::Default))
        .await
        .unwrap();
    assert_eq!(whole.status, 200);
    assert_eq!(body(whole).await, b"abcd");
    let continuation = seen.lock().unwrap()[1].clone();
    assert!(
        continuation.contains("range: bytes=2-3\r\n"),
        "{continuation}"
    );
    assert!(
        continuation.contains("if-range: \"v1\"\r\n"),
        "{continuation}"
    );

    for (path, message) in [
        ("/changed", "Invalid conditional range response"),
        (
            "/not-modified",
            "Incomplete cached response for revalidation",
        ),
    ] {
        let url = format!("{origin}{path}");
        body(
            fetch_stream(&url, ranged(&cache, "bytes=0-1"))
                .await
                .unwrap(),
        )
        .await;
        let error = fetch_stream(&url, options(&cache, CacheMode::Default))
            .await
            .err()
            .unwrap();
        assert_eq!(error.message, message, "{path}");
    }

    // A response policy sees the continuation's own headers and can refuse it.
    let url = format!("{origin}/policy");
    body(
        fetch_stream(&url, ranged(&cache, "bytes=0-1"))
            .await
            .unwrap(),
    )
    .await;
    let deny = |_: &RequestContext, _: &str, headers: &[(String, String)]| {
        !headers
            .iter()
            .any(|(name, value)| name == "content-range" && value == "bytes 2-3/4")
    };
    let mut guarded = options(&cache, CacheMode::Default);
    guarded.response_policy = Some(&deny);
    let error = fetch_stream(&url, guarded).await.err().unwrap();
    assert_eq!(error.message, "Response blocked by native document policy");
}

fn tail_script(_: &str, head: &str) -> &'static str {
    if head.contains("range: bytes=2-3") {
        "HTTP/1.1 206 Partial Content\r\nETag: \"t1\"\r\nCache-Control: max-age=3600\r\nContent-Range: bytes 2-3/4\r\nContent-Length: 2\r\nConnection: close\r\n\r\ncd"
    } else {
        "HTTP/1.1 206 Partial Content\r\nETag: \"t1\"\r\nCache-Control: max-age=3600\r\nX-Fresh: whole\r\nContent-Range: bytes 0-3/4\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd"
    }
}

#[tokio::test]
async fn a_stored_tail_without_a_prefix_refetches_and_stores_the_whole_representation() {
    let (origin, seen) = origin(tail_script).await;
    let cache = HttpCache::default();
    let url = format!("{origin}/tail");
    let tail = fetch_stream(&url, ranged(&cache, "bytes=2-3"))
        .await
        .unwrap();
    assert_eq!(body(tail).await, b"cd");
    // Nothing is stored for byte 0, so the continuation asks for the whole span
    // under If-Range, and the network headers (not the stored part's) are used.
    let observed = Mutex::new(0);
    let observer = |_: &RequestContext, _: &str, _: &[(String, String)]| {
        *observed.lock().unwrap() += 1;
    };
    let mut observed_options = options(&cache, CacheMode::Default);
    observed_options.response_side_effects = Some(&observer);
    let whole = fetch_stream(&url, observed_options).await.unwrap();
    assert_eq!(whole.status, 200);
    assert!(whole
        .headers
        .iter()
        .any(|(name, value)| name == "x-fresh" && value == "whole"));
    assert_eq!(body(whole).await, b"abcd");
    assert_eq!(*observed.lock().unwrap(), 1);
    let head = seen.lock().unwrap()[1].clone();
    assert!(head.contains("range: bytes=0-3\r\n"), "{head}");
    assert!(head.contains("if-range: \"t1\"\r\n"), "{head}");
    // Every byte is now stored and fresh: the next default fetch is a cache hit.
    let cached = fetch_stream(&url, options(&cache, CacheMode::Default))
        .await
        .unwrap();
    assert_eq!(cached.status, 200);
    assert_eq!(body(cached).await, b"abcd");
    assert_eq!(seen.lock().unwrap().len(), 2);
}

fn validator_script(_: &str, head: &str) -> &'static str {
    if head.contains("if-none-match: \"e1\"") {
        "HTTP/1.1 304 Not Modified\r\nETag: \"e1\"\r\nX-Revalidated: yes\r\nConnection: close\r\n\r\n"
    } else {
        "HTTP/1.1 200 OK\r\nETag: \"e1\"\r\nCache-Control: max-age=0\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
    }
}

#[tokio::test]
async fn head_revalidation_and_author_conditionals_follow_fetch_cache_modes() {
    let (origin, seen) = origin(validator_script).await;
    let cache = HttpCache::default();
    let url = format!("{origin}/doc");
    assert_eq!(
        body(
            fetch_stream(&url, options(&cache, CacheMode::Default))
                .await
                .unwrap()
        )
        .await,
        b"body"
    );
    // A stale entry is revalidated for HEAD; the 304 refreshes it and the HEAD
    // response carries the stored metadata with no body.
    let mut head = options(&cache, CacheMode::Default);
    head.method = Some("HEAD".into());
    let response = fetch_stream(&url, head).await.unwrap();
    assert_eq!(response.status, 200);
    assert!(response
        .headers
        .iter()
        .any(|(name, value)| name == "x-revalidated" && value == "yes"));
    assert_eq!(body(response).await, b"");
    assert!(seen.lock().unwrap()[1].starts_with("head /doc "));
    assert!(seen.lock().unwrap()[1].contains("if-none-match: \"e1\""));

    // Fetch 4.5 (HTTP-network-or-cache fetch): an author If-None-Match under the
    // "default" mode switches the request to "no-store" — the 304 reaches the
    // caller untouched and nothing from it is stored or merged.
    let mut conditional = options(&cache, CacheMode::Default);
    conditional
        .headers
        .insert("if-none-match".into(), "\"e1\"".into());
    let response = fetch_stream(&url, conditional).await.unwrap();
    assert_eq!(response.status, 304);
    assert_eq!(seen.lock().unwrap().len(), 3);
    assert!(seen.lock().unwrap()[2].contains("cache-control: no-cache"));
}
