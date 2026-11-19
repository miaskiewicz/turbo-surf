//! Native per-traversable storage controls; no live network.
use turbo_surf_render::PageSession;

async fn page() -> PageSession {
    PageSession::open(
        "<!doctype html><title>Storage</title>",
        "https://storage.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn opener_session_snapshot_is_copied_and_then_isolated() {
    let mut page = page().await;
    let result = page
        .eval_expression(
            r#"(() => {
        sessionStorage.setItem('session', 'before');
        localStorage.setItem('shared', 'before');
        const child = window.open('', '_blank');
        const copied = child.sessionStorage.getItem('session');
        child.sessionStorage.setItem('session', 'child');
        child.localStorage.setItem('shared', 'child');
        sessionStorage.setItem('later', 'opener');
        return JSON.stringify([copied, sessionStorage.getItem('session'),
            child.sessionStorage.getItem('session'), child.sessionStorage.getItem('later'),
            localStorage.getItem('shared')]);
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(result, r#"["before","before","child",null,"child"]"#);
}

#[tokio::test(flavor = "current_thread")]
async fn noopener_creates_an_empty_session_namespace() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval("sessionStorage.setItem('secret', 'opener'); localStorage.setItem('shared','yes'); window.open('', '_blank', 'noopener');")
        .await.unwrap();
    let child = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.target != root.to_string())
        .unwrap();
    let result = page
        .eval_target_expression(
            child.target.parse().unwrap(),
            "JSON.stringify([sessionStorage.getItem('secret'), localStorage.getItem('shared')])",
            false,
            false,
        )
        .await
        .unwrap();
    assert_eq!(result, r#"[null,"yes"]"#);
}

#[tokio::test(flavor = "current_thread")]
async fn nested_popup_copies_its_actual_opener_and_survives_opener_close() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval("sessionStorage.setItem('value','root'); globalThis.child = window.open('', '_blank'); child.sessionStorage.setItem('value','child');")
        .await.unwrap();
    let child = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.target != root.to_string())
        .unwrap();
    page.eval_target_expression(
        child.target.parse().unwrap(),
        "!!window.open('', '_blank')",
        false,
        false,
    )
    .await
    .unwrap();
    let nested = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| {
            info.source_page
                .as_deref()
                .is_some_and(|source| source == child.target)
        })
        .unwrap();
    assert!(page.close_target(root).unwrap());
    let result = page
        .eval_target_expression(
            nested.target.parse().unwrap(),
            "sessionStorage.getItem('value')",
            false,
            false,
        )
        .await
        .unwrap();
    assert_eq!(result, "child");
}

#[tokio::test(flavor = "current_thread")]
async fn navigated_popup_uses_its_actual_first_party_storage_site() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use turbo_surf_render::BrowserStorage;

    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", server.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = "<!doctype html><title>Popup storage</title>";
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    });
    let storage = BrowserStorage::default();
    let mut opener = PageSession::open_with_browsing_state(
        "<!doctype html>",
        "https://opener.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
        storage.clone(),
    )
    .await
    .unwrap();
    let root = opener.auxiliary_root_id().unwrap();
    opener.eval("window.open('', '_blank');").await.unwrap();
    let child = opener
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.target != root.to_string())
        .unwrap();
    let target = child.target.parse().unwrap();
    opener
        .navigate_target(target, &origin, "replace")
        .await
        .unwrap();
    worker.join().unwrap();
    opener
        .eval_target_expression(
            target,
            "localStorage.setItem('native-popup-site', 'shared')",
            false,
            false,
        )
        .await
        .unwrap();
    let mut peer = PageSession::open_with_browsing_state(
        "<!doctype html>",
        &origin,
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
        storage,
    )
    .await
    .unwrap();
    assert_eq!(
        peer.eval_expression("localStorage.getItem('native-popup-site')")
            .await
            .unwrap(),
        "shared"
    );
}
