use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_core::net::{fetch_bytes, fetch_html, FetchOptions};
use turbo_surf_core::request_context::{
    ClientHints, Credentials, Destination, RequestContext, RequestMode,
};

type Captured = Arc<Mutex<Vec<BTreeMap<String, String>>>>;

async fn server() -> (String, Captured) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.windows(4).any(|s| s == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&bytes);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let headers = request
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    Some((name.to_ascii_lowercase(), value.trim().to_string()))
                })
                .collect();
            captured.lock().unwrap().push(headers);
            let extra = match path {
                "/optin" => {
                    "Accept-CH: Sec-CH-UA-Arch, Sec-CH-UA-Bitness, Sec-CH-UA-Full-Version-List, Downlink, RTT\r\n"
                }
                "/clear" => "Accept-CH:\r\n",
                "/repeated" => "Accept-CH: Sec-CH-Prefers-Color-Scheme\r\nAccept-CH: Sec-CH-UA-Arch\r\nAccept-CH: Sec-CH-UA-Full-Version\r\nAccept-CH: Sec-CH-UA-Platform-Version\r\n",
                "/clear-repeated" => "Clear-Site-Data: \"cache\"\r\nClear-Site-Data: \"clientHints\"\r\n",
                "/redirect" => "Location: /after\r\nAccept-CH: Sec-CH-UA-Arch\r\n",
                _ => "",
            };
            let status = if path == "/redirect" {
                "302 Found"
            } else {
                "200 OK"
            };
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n{extra}\r\nok");
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (base, requests)
}

#[tokio::test]
async fn negotiated_hints_and_resource_metadata_reach_the_wire() {
    let (base, requests) = server().await;
    let hints = ClientHints::default();
    fetch_html(
        &format!("{base}/optin"),
        FetchOptions {
            client_hints: Some(&hints),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let context = RequestContext::resource(
        &format!("{base}/page?q=1#fragment"),
        Destination::Script,
        RequestMode::NoCors,
    );
    fetch_html(
        &format!("{base}/script"),
        FetchOptions {
            context: Some(context),
            client_hints: Some(&hints),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let requests = requests.lock().unwrap();
    let cold = &requests[0];
    assert_eq!(cold["sec-fetch-dest"], "document");
    assert_eq!(cold["priority"], "u=0, i");
    assert_eq!(cold["sec-fetch-user"], "?1");
    assert!(!cold.contains_key("sec-ch-ua-arch"));
    let script = &requests[1];
    assert_eq!(script["sec-fetch-dest"], "script");
    assert_eq!(script["priority"], "u=1");
    assert_eq!(script["sec-fetch-mode"], "no-cors");
    assert_eq!(script["sec-fetch-site"], "same-origin");
    assert_eq!(script["accept"], "*/*");
    assert_eq!(script["referer"], format!("{base}/page?q=1"));
    assert_eq!(script["sec-ch-ua-arch"], "\"arm\"");
    assert_eq!(script["sec-ch-ua-bitness"], "\"64\"");
    assert_eq!(script["downlink"], "10");
    assert_eq!(script["rtt"], "50");
    assert!(script["sec-ch-ua-full-version-list"]
        .contains(turbo_surf_core::fingerprint::default_profile().full_version));
    for omitted in ["sec-fetch-user", "upgrade-insecure-requests", "origin"] {
        assert!(
            !script.contains_key(omitted),
            "unexpected {omitted}: {script:?}"
        );
    }
}

#[tokio::test]
async fn hints_do_not_leak_to_a_new_origin_and_empty_accept_ch_clears_them() {
    let (base, requests) = server().await;
    let (other, other_requests) = server().await;
    let hints = ClientHints::default();
    for url in [
        format!("{base}/optin"),
        format!("{other}/"),
        format!("{base}/clear"),
        format!("{base}/"),
    ] {
        fetch_html(
            &url,
            FetchOptions {
                client_hints: Some(&hints),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    assert!(!other_requests.lock().unwrap()[0].contains_key("sec-ch-ua-arch"));
    let requests = requests.lock().unwrap();
    assert!(requests[1].contains_key("sec-ch-ua-arch"));
    assert!(!requests[2].contains_key("sec-ch-ua-arch"));
}

#[tokio::test]
async fn redirects_negotiate_hints_before_next_hop_and_omit_credentials_is_enforced() {
    let (base, requests) = server().await;
    let hints = ClientHints::default();
    fetch_html(
        &format!("{base}/redirect"),
        FetchOptions {
            client_hints: Some(&hints),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut jar = CookieJar::new();
    jar.set_from_response(&base, &["secret=server; Path=/".into()], 0.0);
    let mut context = RequestContext::resource(
        &format!("{base}/page"),
        Destination::Empty,
        RequestMode::Cors,
    );
    context.credentials = Credentials::Omit;
    fetch_html(
        &format!("{base}/post"),
        FetchOptions {
            context: Some(context),
            method: Some("POST".into()),
            jar: Some(&mut jar),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let requests = requests.lock().unwrap();
    assert!(!requests[0].contains_key("sec-ch-ua-arch"));
    assert_eq!(requests[1]["sec-ch-ua-arch"], "\"arm\"");
    assert_eq!(requests[2]["origin"], base);
    assert_eq!(requests[2]["sec-fetch-dest"], "empty");
    assert_eq!(requests[2]["priority"], "u=1, i");
    assert_eq!(requests[2]["sec-fetch-mode"], "cors");
    assert!(!requests[2].contains_key("cookie"));
}

#[tokio::test]
async fn binary_resource_redirects_use_origin_aware_request_context() {
    let (base, requests) = server().await;
    let hints = ClientHints::default();
    let (final_url, bytes, status) = fetch_bytes(
        &format!("{base}/redirect"),
        FetchOptions {
            context: Some(RequestContext::resource(
                &format!("{base}/page"),
                Destination::Image,
                RequestMode::NoCors,
            )),
            client_hints: Some(&hints),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(status, 200);
    assert_eq!(bytes, b"ok");
    assert_eq!(final_url, format!("{base}/after"));
    let requests = requests.lock().unwrap();
    assert_eq!(requests[0]["sec-fetch-dest"], "image");
    assert_eq!(requests[0]["priority"], "u=2, i");
    assert!(!requests[0].contains_key("sec-ch-ua-arch"));
    assert_eq!(requests[1]["sec-ch-ua-arch"], "\"arm\"");
    assert!(!requests[1].contains_key("sec-fetch-user"));
}

#[tokio::test]
async fn repeated_hint_and_clear_site_data_fields_are_combined() {
    let (base, requests) = server().await;
    let hints = ClientHints::default();
    for path in ["/repeated", "/after", "/clear-repeated", "/after"] {
        fetch_html(
            &format!("{base}{path}"),
            FetchOptions {
                client_hints: Some(&hints),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let profile = turbo_surf_core::fingerprint::default_profile();
    let requests = requests.lock().unwrap();
    assert!(!requests[0].contains_key("sec-ch-ua-arch"));
    let negotiated = &requests[1];
    assert_eq!(negotiated["sec-ch-prefers-color-scheme"], "light");
    assert_eq!(
        negotiated["sec-ch-ua-arch"],
        format!("\"{}\"", profile.architecture())
    );
    assert_eq!(
        negotiated["sec-ch-ua-full-version"],
        format!("\"{}\"", profile.full_version)
    );
    assert_eq!(
        negotiated["sec-ch-ua-platform-version"],
        format!("\"{}\"", profile.platform_version())
    );
    assert!(requests[2].contains_key("sec-ch-ua-arch"));
    assert!(!requests[3].contains_key("sec-ch-ua-arch"));
    assert!(!requests[3].contains_key("sec-ch-ua-platform-version"));
}

#[tokio::test]
async fn browsing_session_reuses_connection_between_navigation_and_resource() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        // Only one connection is accepted. The test times out if the second
        // request creates another client instead of using the session pool.
        let (mut socket, _) = listener.accept().await.unwrap();
        for _ in 0..2 {
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let n = socket.read(&mut chunk).await.unwrap();
                assert_ne!(n, 0, "session closed its reusable connection");
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.windows(4).any(|s| s == b"\r\n\r\n") {
                    break;
                }
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nAccept-CH: Sec-CH-UA-Arch\r\n\r\nok").await.unwrap();
        }
    });
    let hints = ClientHints::default();
    for path in ["/page", "/script"] {
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            fetch_html(
                &format!("{base}{path}"),
                FetchOptions {
                    client_hints: Some(&hints.clone()),
                    context: Some(if path == "/page" {
                        RequestContext::default()
                    } else {
                        RequestContext::resource(&base, Destination::Script, RequestMode::NoCors)
                    }),
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap()
        .unwrap();
    }
    server.await.unwrap();
}
