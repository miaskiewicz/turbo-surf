//! Independent PageSessions must keep native document state within their isolate.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn two_live_sessions_keep_document_identity_and_survive_peer_close() {
    let mut first = PageSession::open(
        "<!doctype html><body><p id='first'>one</p></body>",
        "https://first.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    first
        .eval_expression("globalThis.held=document.querySelector('p'); held.expando=17; 'ready'")
        .await
        .unwrap();
    let mut second = PageSession::open(
        "<!doctype html><body><p id='second'>two</p></body>",
        "https://second.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    assert_eq!(
        second
            .eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "two"
    );
    const CHECK: &str = "JSON.stringify([document.URL,document.body.textContent,held===document.querySelector('p'),held.expando])";
    assert_eq!(
        first.eval_expression(CHECK).await.unwrap(),
        r#"["https://first.test/","one",true,17]"#
    );
    drop(second);
    assert_eq!(
        first.eval_expression(CHECK).await.unwrap(),
        r#"["https://first.test/","one",true,17]"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn interleaved_session_polls_restore_isolate_and_native_document_state() {
    let mut first = PageSession::open("<body>one", "https://first.test/", "", "", 5000)
        .await
        .unwrap();
    let mut second = PageSession::open("<body>two", "https://second.test/", "", "", 5000)
        .await
        .unwrap();
    let (one, two) = tokio::join!(
        first.eval_expression(
            "new Promise(resolve=>setTimeout(()=>resolve(document.body.textContent),10))"
        ),
        second.eval_expression(
            "new Promise(resolve=>setTimeout(()=>resolve(document.body.textContent),1))"
        )
    );
    assert_eq!(one.unwrap(), "one");
    assert_eq!(two.unwrap(), "two");
    assert!(first.serialize().contains("one"));
    assert!(second.serialize().contains("two"));
}

#[tokio::test(flavor = "current_thread")]
async fn closing_older_session_and_collecting_peer_preserves_live_native_identity() {
    let first = PageSession::open("<body>one", "https://first.test/", "", "", 5000)
        .await
        .unwrap();
    let mut second = PageSession::open("<body><p>two</p>", "https://second.test/", "", "", 5000)
        .await
        .unwrap();
    second
        .eval_expression("globalThis.held=document.querySelector('p');held.marker=23;'ready'")
        .await
        .unwrap();
    first.close();
    second.collect_message_port_garbage_for_testing();
    assert_eq!(
        second
            .eval_json_expression("[document.URL,held===document.querySelector('p'),held.marker]")
            .await
            .unwrap(),
        r#"["https://second.test/",true,23]"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cached_and_stateless_runtimes_do_not_reset_a_live_session() {
    let mut page = PageSession::open(
        "<body><p>session</p>",
        "https://session.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression("globalThis.held=document.querySelector('p');held.marker=31;'ready'")
        .await
        .unwrap();
    assert_eq!(
        turbo_surf_render::run_with_dom("<body>cached", "document.body.textContent").unwrap(),
        "cached"
    );
    let html =
        turbo_surf_render::render_html("<body>sync", "document.body.append(' result')").unwrap();
    assert!(html.contains("sync result"), "{html}");
    let (snapshot, identity) = tokio::join!(
        turbo_surf_render::render_page("<body>async", "https://async.test/", "setTimeout(()=>document.body.append(' result'),10)"),
        page.eval_json_expression("new Promise(resolve=>setTimeout(()=>resolve([document.URL,held===document.querySelector('p'),held.marker]),1))")
    );
    assert!(snapshot.unwrap().contains("async result"));
    assert_eq!(identity.unwrap(), r#"["https://session.test/",true,31]"#);
    assert_eq!(
        turbo_surf_render::run_with_dom("<body>replacement", "document.body.textContent").unwrap(),
        "replacement"
    );
    assert_eq!(
        page.eval_expression("held.textContent").await.unwrap(),
        "session"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_open_polls_preserve_document_observers_and_timers() {
    let (first, second) = tokio::join!(
        PageSession::open("<body>one<script>setTimeout(()=>document.body.setAttribute('done','first'),10)</script>", "https://first.test/", "", "", 5000),
        PageSession::open("<body>two<script>setTimeout(()=>document.body.setAttribute('done','second'),1)</script>", "https://second.test/", "", "", 5000)
    );
    let mut first = first.unwrap();
    let mut second = second.unwrap();
    assert_eq!(
        first
            .eval_expression("document.body.getAttribute('done')")
            .await
            .unwrap(),
        "first"
    );
    assert_eq!(
        second
            .eval_expression("document.body.getAttribute('done')")
            .await
            .unwrap(),
        "second"
    );
    first.eval_expression("globalThis.changes=[];new MutationObserver(rows=>changes.push(rows[0].target.textContent)).observe(document.body,{childList:true});'ready'").await.unwrap();
    second
        .eval_expression("document.body.append('foreign');'changed'")
        .await
        .unwrap();
    assert_eq!(first.eval_json_expression("changes").await.unwrap(), "[]");
    first
        .eval_expression("document.body.append('own');'changed'")
        .await
        .unwrap();
    assert_eq!(first.eval_expression("changes.length").await.unwrap(), "1");
}

#[tokio::test(flavor = "current_thread")]
async fn canceling_pending_session_work_restores_the_peer_and_its_own_runtime() {
    let mut first = PageSession::open("<body>one", "https://first.test/", "", "", 5000)
        .await
        .unwrap();
    let mut second = PageSession::open("<body>two", "https://second.test/", "", "", 5000)
        .await
        .unwrap();
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        assert!(stream.read(&mut bytes).unwrap() > 0);
        wait.recv().unwrap();
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\nok");
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        first.eval_expression(&format!(
            "fetch('http://{address}/pending').then(r=>r.text())"
        )),
    )
    .await;
    release.send(()).unwrap();
    server.join().unwrap();
    assert!(
        result.is_err(),
        "pending network evaluation should be canceled by the host: {result:?}"
    );
    assert_eq!(
        second
            .eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "two"
    );
    assert_eq!(
        first
            .eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "one"
    );
}
