//! Page fetches use the browsing session's request state, rather than navigation defaults.
use std::collections::BTreeMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn page_fetch_uses_negotiated_hints_and_browser_owned_headers() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..3 {
            let (mut socket, _) = listener.accept().await.unwrap();
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
            let headers: BTreeMap<String, String> = request
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    Some((name.to_ascii_lowercase(), value.trim().to_string()))
                })
                .collect();
            requests.push(headers);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
        }
        requests
    });
    let hints = ClientHints::default();
    hints.observe(&base, Some("sec-ch-ua-arch"), false);
    let mut jar = CookieJar::new();
    jar.set_from_response(&base, &["secret=server; HttpOnly; Path=/".into()], 0.0);
    let mut page = PageSession::open_with_request_state(
        "<body></body>",
        &format!("{base}/page?q=1#fragment"),
        &jar.storage_state(),
        "",
        5000,
        "",
        hints,
    )
    .await
    .unwrap();
    page.eval(r#"
        fetch('/script.js', { destination:'script' })
        .then(() => fetch('/post', { method:'POST', credentials:'omit',
            headers:{Origin:'https://spoof.test',Cookie:'secret=guest','Sec-Fetch-Mode':'navigate'} }))
        .then(() => fetch('/api'))
        .then(() => {globalThis.__RESULT='done'});
    "#).await.unwrap();
    page.close();
    let requests = tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    let script = &requests[0];
    assert_eq!(script["sec-fetch-dest"], "script");
    assert_eq!(script["sec-fetch-mode"], "no-cors");
    assert_eq!(script["sec-ch-ua-arch"], "\"arm\"");
    assert_eq!(script["cookie"], "secret=server");
    assert_eq!(script["referer"], format!("{base}/page?q=1"));
    for omitted in ["sec-fetch-user", "upgrade-insecure-requests", "origin"] {
        assert!(
            !script.contains_key(omitted),
            "unexpected {omitted}: {script:?}"
        );
    }
    let post = &requests[1];
    assert_eq!(post["origin"], base);
    assert_eq!(post["sec-fetch-mode"], "cors");
    assert!(!post.contains_key("cookie"));
    let api = &requests[2];
    assert_eq!(api["sec-fetch-dest"], "empty");
    assert_eq!(api["sec-fetch-mode"], "cors");
    assert_eq!(api["cookie"], "secret=server");
    assert!(!api.contains_key("origin"));
}
