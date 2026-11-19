//! Accepted forms belong to the actor even when a host read interrupts its poll.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn form_admission_survives_a_dropped_background_turn_and_readonly_query() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (entered, received) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "native form was not admitted");
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buffer = [0; 4096];
            let size = stream.read(&mut buffer).unwrap();
            let wire = String::from_utf8_lossy(&buffer[..size]);
            assert!(wire.starts_with("POST /result HTTP/1.1\r\n"), "{wire}");
            let body = "<p id='done'>destination</p>";
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.flush().unwrap();
            entered.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(3)).unwrap();
            stream.write_all(body.as_bytes()).unwrap();
            return 1;
        }
    });
    let mut page = PageSession::open(
        "<form method=post action='/result'><input name=q value=once></form>",
        &origin,
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let target = page.auxiliary_root_id().unwrap();
    page.eval_target_engine(
        target,
        "document.forms[0].submit();globalThis.__RESULT='queued';",
        false,
    )
    .await
    .unwrap();
    let mut work = Box::pin(page.poll_background_work());
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            arrived = received => arrived.unwrap(),
            result = &mut work => panic!("background turn finished before the held response: {result:?}"),
        }
    }).await.unwrap();
    drop(work);
    assert!(
        page.has_pending_auxiliary_work(),
        "accepted request must remain actor-owned after interruption"
    );
    assert_eq!(
        page.read_target_engine(target, "return document.forms.length;")
            .unwrap(),
        "1"
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while page.has_pending_auxiliary_work() {
            page.poll_background_work().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(
        page.read_target_engine(
            target,
            "return document.getElementById('done').textContent;"
        )
        .unwrap(),
        "\"destination\""
    );
    assert_eq!(page.auxiliary_root_id().unwrap(), target);
    let records: serde_json::Value = serde_json::from_str(&page.drain_network_log()).unwrap();
    let responses = records
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["event"] == "response" && entry["url"] == format!("{origin}/result"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 1, "no canceled or replayed form attempt");
    assert_eq!(responses[0]["status"], 200);
    page.close();
    assert_eq!(server.join().unwrap(), 1, "never replay the form request");
}
