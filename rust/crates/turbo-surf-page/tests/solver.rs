//! TurboNavigator with a solver configured from the environment: a detected wall is
//! solved once and the page re-fetched with the token's cookies and headers. One test:
//! it sets process environment variables.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::crawl::Navigator;
use turbo_surf_page::TurboNavigator;

const WALL: &str = "<title>Just a moment...</title><script src='/cdn-cgi/challenge-platform/x.js'></script>";

// Serves the Cloudflare wall until a request carries the solver's cookie and header,
// then the real page; every request line + cookie/header is forwarded to `log`.
async fn spawn_walled_site() -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            let cleared = req.contains("\r\ncookie: cf_clearance=ok; z=2\r\n")
                && req.contains("\r\nx-token: t1\r\n");
            let _ = tx.send(format!(
                "{} cleared={cleared}",
                req.lines().next().unwrap_or("")
            ));
            let body = if cleared {
                "<title>Real</title><a href='/next'>n</a>"
            } else {
                WALL
            };
            let resp = format!(
                "HTTP/1.1 {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                if cleared { "200 OK" } else { "403 Forbidden" },
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    (base, rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_configured_solver_clears_a_wall_and_refetches_with_the_token() {
    let (base, mut log) = spawn_walled_site().await;
    for k in ["HYPER_API_KEY", "SCRAPFLY_API_KEY", "TURBO_SURF_PROXY"] {
        std::env::remove_var(k);
    }
    // No solver configured: the wall comes back as-is.
    std::env::remove_var("TURBO_SURF_SOLVER");
    let plain = TurboNavigator::default().with_solver_from_env();
    let nav = plain.goto(&format!("{base}/")).await.unwrap();
    assert_eq!((nav.status, nav.title.as_str()), (403, "Just a moment..."));
    assert_eq!(log.recv().await.unwrap(), "get / http/1.1 cleared=false");

    // The browser sidecar solver hands back cookies + a header.
    std::env::set_var("TURBO_SURF_SOLVER", "browser");
    std::env::set_var(
        "TURBO_SURF_BROWSER_CMD",
        r#"cat >/dev/null; echo '{"cookies":{"cf_clearance":"ok","z":"2"},"headers":{"X-Token":"t1"}}'"#,
    );
    let solving = TurboNavigator::default()
        .with_item_selector(Some("a".into()))
        .with_solver_from_env();
    let nav = solving.goto(&format!("{base}/")).await.unwrap();
    assert_eq!((nav.status, nav.title.as_str()), (200, "Real"));
    assert_eq!(nav.links, vec![format!("{base}/next")]);
    assert_eq!(nav.items, 1);
    assert_eq!(log.recv().await.unwrap(), "get / http/1.1 cleared=false");
    assert_eq!(log.recv().await.unwrap(), "get / http/1.1 cleared=true");

    // A failing solver leaves the wall response in place (one fetch, no retry).
    std::env::set_var("TURBO_SURF_BROWSER_CMD", "cat >/dev/null; exit 1");
    let failing = TurboNavigator::default().with_solver_from_env();
    let nav = failing.goto(&format!("{base}/")).await.unwrap();
    assert_eq!(nav.status, 403);
    assert_eq!(log.recv().await.unwrap(), "get / http/1.1 cleared=false");
    assert!(log.try_recv().is_err());
    std::env::remove_var("TURBO_SURF_SOLVER");
    std::env::remove_var("TURBO_SURF_BROWSER_CMD");
}
