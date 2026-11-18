//! Native transport admission after actual document-owned cancellation.
use super::*;
use crate::runtime::{private_infrastructure, ActiveDocument, PageSession};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn registered_job(page: &mut PageSession) -> (u32, Arc<Job>) {
    let _activation = page.rt.activation_key().enter();
    let page_id = page.auxiliary_root_id().unwrap().to_string();
    let context = private_infrastructure::context(&mut page.rt).unwrap();
    let state = page.rt.op_state();
    deno_core::scope!(scope, page.rt);
    let context = v8::Local::new(scope, context);
    let document = context
        .get_slot::<ActiveDocument>()
        .unwrap()
        .0
        .to_local(scope)
        .unwrap();
    let id = prepare(scope, &mut state.borrow_mut(), document, None).unwrap();
    let retained = job(&state.borrow(), id).unwrap();
    assert_eq!(retained.frame.page_id.as_deref(), Some(page_id.as_str()));
    assert_eq!(
        state
            .borrow()
            .borrow::<Jobs>()
            .owners
            .get(&id)
            .unwrap()
            .to_local(scope),
        Some(context),
        "the registered job must belong to the actual selected Root document"
    );
    (id, retained)
}

async fn headers(socket: &mut tokio::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let mut chunk = [0; 1024];
        let length = socket.read(&mut chunk).await.unwrap();
        assert!(length > 0, "request headers must complete");
        bytes.extend_from_slice(&chunk[..length]);
        assert!(bytes.len() <= 8192);
    }
    String::from_utf8(bytes)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_document_job_refuses_late_transport_while_live_job_accepts_real_bytes() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (closed, closure) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut denied, _) = listener.accept().await.unwrap();
        assert_eq!(headers(&mut denied).await, "/cancelled");
        denied.write_all(b"HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: 4\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut byte = [0];
        let count = tokio::time::timeout(std::time::Duration::from_secs(3), denied.read(&mut byte))
            .await
            .expect("rejected transport must close without receiving a body")
            .unwrap();
        assert_eq!(count, 0);
        closed.send(()).unwrap();
        let (mut accepted, _) = listener.accept().await.unwrap();
        assert_eq!(headers(&mut accepted).await, "/accepted");
        accepted.write_all(b"HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngood").await.unwrap();
    });
    let mut page = PageSession::open("<body>healthy", &base, "", "", 5000)
        .await
        .unwrap();
    let (cancelled_id, cancelled) = registered_job(&mut page);
    let late = turbo_surf_core::net::fetch_stream(&format!("{base}/cancelled"), Default::default())
        .await
        .unwrap();
    release_cancel(&mut page.rt.op_state().borrow_mut(), cancelled_id);
    assert!(job(&page.rt.op_state().borrow(), cancelled_id).is_none());
    assert!(*cancelled.cancel.borrow());
    assert!(!install(&cancelled, late.body).await);
    assert!(cancelled.body.lock().await.is_none());
    closure.await.unwrap();

    let (live_id, live) = registered_job(&mut page);
    let received =
        turbo_surf_core::net::fetch_stream(&format!("{base}/accepted"), Default::default())
            .await
            .unwrap();
    assert!(install(&live, received.body).await);
    let mut installed = live.body.lock().await;
    let body = installed.as_mut().unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next_chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, b"good");
    drop(installed);
    release_cancel(&mut page.rt.op_state().borrow_mut(), live_id);
    assert!(job(&page.rt.op_state().borrow(), live_id).is_none());
    assert_eq!(
        page.eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "healthy"
    );
    server.await.unwrap();
    page.close();
}
