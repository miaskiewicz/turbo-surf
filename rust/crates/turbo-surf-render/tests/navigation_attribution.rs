//! Native attribution follows accepted request plans, not the HTTP method or DOM shape.
use std::sync::{Arc, Mutex};
use turbo_surf_render::PageSession;

async fn finish_root(page: &mut PageSession) {
    let root = page.auxiliary_root_id().unwrap().to_string();
    let log = page.network_event_log();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while log.target_network_state(&root)["pendingNavigation"] == true {
            page.poll_root_navigation_work().await.unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn root_form_counter_distinguishes_get_submission_from_script_navigation() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let wire = requests.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let wire = wire.clone();
            tokio::spawn(async move {
                let mut bytes = [0; 4096];
                let count = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..count]);
                let line = request.lines().next().unwrap();
                let path = line.split_whitespace().nth(1).unwrap();
                wire.lock().unwrap().push(line.to_owned());
                let body = format!("<body>installed {path}</body>");
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    let mut page = PageSession::open(
        "<body><form action=/form method=get><input name=q value=kept></form></body>",
        &base,
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    assert_eq!(page.navigation_snapshot().0, 0);
    assert_eq!(page.form_navigation_count(), 0);
    page.eval_expression("document.querySelector('form').submit();void 0")
        .await
        .unwrap();
    finish_root(&mut page).await;
    assert_eq!(page.navigation_snapshot().0, 1);
    assert_eq!(page.form_navigation_count(), 1);
    let root = page.auxiliary_root_id().unwrap();
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("/form?q=kept"));
    page.eval_expression("location.assign('/script');void 0")
        .await
        .unwrap();
    finish_root(&mut page).await;
    assert_eq!(page.navigation_snapshot().0, 2);
    assert_eq!(page.form_navigation_count(), 1);
    assert!(page.serialize_target(root).unwrap().contains("/script"));
    assert_eq!(
        *requests.lock().unwrap(),
        vec!["GET /form?q=kept HTTP/1.1", "GET /script HTTP/1.1"]
    );
    page.close();
    server.abort();
}
