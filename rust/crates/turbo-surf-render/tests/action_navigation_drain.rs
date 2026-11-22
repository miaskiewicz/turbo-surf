//! Action completion awaits its navigation commit, not replacement background work.
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn async_action_navigation_does_not_wait_for_retiring_document_background_fetch() {
    action_navigation_with_held_fetch(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn same_document_action_commit_does_not_wait_for_background_fetch() {
    action_navigation_with_held_fetch(true).await;
}

async fn action_navigation_with_held_fetch(same_document: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}/", listener.local_addr().unwrap());
    let release = Arc::new(tokio::sync::Notify::new());
    let signal = release.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let release = signal.clone();
            tokio::spawn(async move {
                let mut request = [0; 4096];
                let n = socket.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..n]);
                let path = request.split_whitespace().nth(1).unwrap();
                if path == "/held" {
                    release.notified().await;
                }
                let body = if path == "/next" {
                    "<p id=committed>new document</p>"
                } else {
                    "ok"
                };
                let wire=format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                let _ = socket.write_all(wire.as_bytes()).await;
            });
        }
    });
    let html = if same_document {
        r#"<button onclick="fetch('/auth').then(()=>{history.pushState({},'', '/ready');fetch('/held')})">Go</button>"#
    } else {
        r#"<button onclick="fetch('/auth').then(()=>{location.href='/next';fetch('/held')})">Go</button>"#
    };
    let mut page = PageSession::open(html, &origin, "", "", 5000)
        .await
        .unwrap();
    let target = page.auxiliary_root_id().unwrap();
    let completed = {
        let action = page.dispatch_target_engine(
            target,
            "document.querySelector('button').click();globalThis.__RESULT='clicked';",
        );
        tokio::pin!(action);
        match tokio::time::timeout(Duration::from_millis(700), &mut action).await {
            Ok(result) => {
                assert_eq!(result.unwrap(), "clicked");
                true
            }
            Err(_) => {
                release.notify_waiters();
                let _ = action.await;
                false
            }
        }
    };
    if same_document {
        assert_eq!(
            page.read_target_engine(target, "return location.pathname;")
                .unwrap(),
            "\"/ready\""
        );
    } else {
        assert!(page
            .serialize_target(target)
            .unwrap()
            .contains("new document"));
    }
    page.close();
    server.abort();
    assert!(
        completed,
        "action must finish at navigation commit while retiring document background fetch is held"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn action_without_navigation_still_drains_its_own_fetch() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}/", listener.local_addr().unwrap());
    let release = Arc::new(tokio::sync::Notify::new());
    let released = release.clone();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await;
        released.notified().await;
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await;
    });
    let mut page = PageSession::open("<p>original</p>", &origin, "", "", 5000)
        .await
        .unwrap();
    let target = page.auxiliary_root_id().unwrap();
    {
        let action = page.dispatch_target_engine(
            target,
            "try{history.pushState({},'', 'https://different.invalid/');throw new Error('accepted invalid history')}catch(error){if(error.name!=='SecurityError')throw error;globalThis.historyRejected=true;}fetch('/held').then(()=>globalThis.finished=true);globalThis.__RESULT='dispatched';",
        );
        tokio::pin!(action);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut action)
                .await
                .is_err()
        );
        release.notify_one();
        assert_eq!(action.await.unwrap(), "dispatched");
    }
    assert_eq!(
        page.read_target_engine(target, "return globalThis.finished;")
            .unwrap(),
        "true"
    );
    assert_eq!(
        page.read_target_engine(target, "return globalThis.historyRejected;")
            .unwrap(),
        "true"
    );
    page.close();
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn action_guest_execution_keeps_its_runaway_budget() {
    let mut page = PageSession::open("<p>original</p>", "http://localhost/", "", "", 200)
        .await
        .unwrap();
    let target = page.auxiliary_root_id().unwrap();
    let error = page
        .dispatch_target_engine(target, "while(true){}")
        .await
        .unwrap_err();
    assert!(error.contains("execution budget"), "{error}");
    page.close();
}
