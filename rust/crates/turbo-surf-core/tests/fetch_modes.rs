//! Fetch redirect modes, cache reuse shortcuts, request hooks and header
//! admission against a scripted localhost origin (Fetch Standard behaviour).
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::http_cache::{CacheMode, HttpCache};
use turbo_surf_core::net::{
    fetch_html, fetch_stream, BeforeRequestFuture, FetchOptions, HttpError, RedirectMode,
};
use turbo_surf_core::request_context::{Destination, RequestContext, RequestMode};

fn route(path: &str) -> &'static str {
    match path {
        "/redirect" => "HTTP/1.1 302 Found\r\nLocation: /landing\r\nReferrer-Policy: no-referrer\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "/empty" => "HTTP/1.1 204 No Content\r\nCache-Control: max-age=60\r\nConnection: close\r\n\r\n",
        _ => "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nCache-Control: max-age=60\r\nContent-Length: 4\r\nConnection: close\r\n\r\npage",
    }
}

// Records every request (head plus whatever body arrived with it).
async fn origin() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut bytes = [0; 4096];
                    let n = socket.read(&mut bytes).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&bytes[..n]);
                }
                // Read the body announced by Content-Length, however it was split.
                let head_end = request.windows(4).position(|p| p == b"\r\n\r\n").unwrap() + 4;
                let length = String::from_utf8_lossy(&request[..head_end])
                    .to_ascii_lowercase()
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: ")?.trim().parse().ok())
                    .unwrap_or(0usize);
                while request.len() < head_end + length {
                    let mut bytes = [0; 4096];
                    let n = socket.read(&mut bytes).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&bytes[..n]);
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
                log.lock().unwrap().push(text.to_ascii_lowercase());
                let _ = socket.write_all(route(&path).as_bytes()).await;
            });
        }
    });
    (origin, seen)
}

#[tokio::test]
async fn redirect_modes_error_manual_and_follow_with_a_new_referrer_policy() {
    let (origin, seen) = origin().await;
    let url = format!("{origin}/redirect");
    let error = fetch_stream(
        &url,
        FetchOptions {
            redirect: Some(RedirectMode::Error),
            ..Default::default()
        },
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.message, "Request redirect mode is error");
    let manual = fetch_stream(
        &url,
        FetchOptions {
            redirect: Some(RedirectMode::Manual),
            allow_non_html: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!((manual.status, manual.redirected), (302, false));
    assert_eq!(manual.final_url, url);
    // Following: the hop's Referrer-Policy (no-referrer) governs the next request.
    let context = RequestContext::resource(
        &format!("{origin}/app"),
        Destination::Empty,
        RequestMode::Cors,
    );
    let followed = fetch_stream(
        &url,
        FetchOptions {
            context: Some(context),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(followed.redirected);
    assert_eq!(followed.final_url, format!("{origin}/landing"));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    assert!(
        seen[2].contains(&format!("referer: {origin}/app\r\n")),
        "{}",
        seen[2]
    );
    assert!(seen[3].starts_with("get /landing "));
    assert!(!seen[3].contains("referer:"), "{}", seen[3]);
}

#[tokio::test]
async fn cache_shortcuts_serve_head_and_204_without_the_network() {
    let (origin, seen) = origin().await;
    let cache = HttpCache::default();
    let options = |mode, method: &str| FetchOptions {
        http_cache: Some(&cache),
        cache_mode: mode,
        method: Some(method.into()),
        allow_non_html: true,
        ..Default::default()
    };
    let mut page = fetch_stream(
        &format!("{origin}/page"),
        options(CacheMode::Default, "GET"),
    )
    .await
    .unwrap();
    while page.body.next_chunk().await.unwrap().is_some() {}
    // A fresh stored GET answers HEAD with its metadata and no body.
    let mut head = fetch_stream(
        &format!("{origin}/page"),
        options(CacheMode::Default, "HEAD"),
    )
    .await
    .unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.body.next_chunk().await.unwrap(), None);
    // A 204 is stored as soon as its headers arrive (it has no body).
    let empty = fetch_stream(
        &format!("{origin}/empty"),
        options(CacheMode::Default, "GET"),
    )
    .await
    .unwrap();
    assert_eq!(empty.status, 204);
    let cached = fetch_stream(
        &format!("{origin}/empty"),
        options(CacheMode::OnlyIfCached, "GET"),
    )
    .await
    .unwrap();
    assert_eq!(cached.status, 204);
    // only-if-cached never issues a range request.
    let mut ranged = options(CacheMode::OnlyIfCached, "GET");
    ranged.headers.insert("range".into(), "bytes=0-1".into());
    let error = fetch_stream(&format!("{origin}/page"), ranged)
        .await
        .err()
        .unwrap();
    assert_eq!(error.message, "HTTP cache miss");
    assert_eq!(seen.lock().unwrap().len(), 2);
    // A response that cannot be stored has no background fill to start or detach.
    let uncached = fetch_stream(&format!("{origin}/page"), FetchOptions::default())
        .await
        .unwrap();
    assert!(!uncached.body.has_background_cache());
    let (_owner, watch) = tokio::sync::watch::channel(false);
    let mut body = uncached.body.background_cache(watch);
    assert!(!body.has_background_cache());
    assert!(!body.detach_cache_consumer());
    assert_eq!(body.background_status(), None);
    assert_eq!(body.next_chunk().await.unwrap().unwrap(), b"page");
}

#[tokio::test]
async fn hooks_shared_clients_and_credentialless_cookie_admission() {
    let (origin, seen) = origin().await;
    // before_request sees each hop and can veto it before any I/O.
    let calls = Mutex::new(Vec::new());
    let hook = |_: RequestContext, url: String, method: String, _| -> BeforeRequestFuture {
        calls.lock().unwrap().push(format!("{method} {url}"));
        let veto = url.ends_with("/vetoed");
        Box::pin(async move {
            if veto {
                Err(HttpError {
                    message: "vetoed by host".into(),
                    code: turbo_surf_core::net::ErrorCode::Network,
                })
            } else {
                Ok(())
            }
        })
    };
    let error = fetch_stream(
        &format!("{origin}/vetoed"),
        FetchOptions {
            before_request: Some(&hook),
            ..Default::default()
        },
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.message, "vetoed by host");
    fetch_stream(
        &format!("{origin}/redirect"),
        FetchOptions {
            before_request: Some(&hook),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        [
            format!("GET {origin}/vetoed"),
            format!("GET {origin}/redirect"),
            format!("GET {origin}/landing"),
        ]
    );
    assert_eq!(seen.lock().unwrap().len(), 2);

    // A caller-supplied client is used as is; raw body bytes are sent verbatim.
    let client = turbo_surf_core::net::build_client();
    let page = fetch_html(
        &format!("{origin}/shared"),
        FetchOptions {
            client: Some(&client),
            method: Some("PUT".into()),
            body_bytes: Some(vec![0x00, 0x7f]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.html, "page");
    let put = seen.lock().unwrap()[2].clone();
    assert!(put.starts_with("put /shared "), "{put}");
    assert!(put.contains("content-length: 2\r\n"), "{put}");
    assert!(put.ends_with("\r\n\r\n\u{0}\u{7f}"), "{put:?}");

    // Credentialless (COEP) no-cors requests to another site drop author cookies.
    let mut context = RequestContext::resource(
        "https://elsewhere.test/",
        Destination::Image,
        RequestMode::NoCors,
    );
    context.credentialless = true;
    let mut headers = std::collections::BTreeMap::new();
    headers.insert("cookie".to_owned(), "sid=1".to_owned());
    fetch_stream(
        &format!("{origin}/image"),
        FetchOptions {
            context: Some(context),
            headers,
            bypass_consent: true,
            allow_non_html: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let image = seen.lock().unwrap()[3].clone();
    assert!(image.starts_with("get /image "), "{image}");
    assert!(!image.contains("cookie:"), "{image}");

    assert!(turbo_surf_core::net::report_client_with_trust(b"not a certificate").is_err());
}
