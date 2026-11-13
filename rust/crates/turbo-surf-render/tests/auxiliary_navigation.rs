//! Real Render/V8 controls live in a separate integration-test process.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use turbo_surf_render::{AuxiliaryPageEvent, PageSession};

#[tokio::test(flavor = "current_thread")]
async fn failed_backing_install_retires_only_addressed_real_page() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    std::env::set_var(
        "TURBO_SURF_ALLOWED_ORIGINS",
        serde_json::to_string(&[&origin]).unwrap(),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !server_stop.load(Ordering::Acquire) && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0; 4096];
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let _ = stream.read(&mut buffer);
                    let body = "<!doctype html><title>Fetched real document</title>";
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("fixture listener failed: {error}"),
            }
        }
    });
    let mut page = PageSession::open("<!doctype html><title>Root</title>", &origin, "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.eval("globalThis.spare = window.open('about:blank', '_blank');")
        .await
        .unwrap();
    let log = page.auxiliary_event_log();
    let child = log
        .pages()
        .into_iter()
        .find(|info| info.target != root.to_string())
        .expect("native open must create a real child");
    let child_id = child.target.parse().unwrap();
    log.take();
    page.fail_next_auxiliary_backing_for_testing(root).unwrap();
    let error = page
        .navigate_target(root, &format!("{origin}/fault"), "replace")
        .await
        .unwrap_err();
    assert!(
        error.contains("Injected native auxiliary backing"),
        "{error}"
    );
    assert!(log.info(&root.to_string()).unwrap().closed);
    assert!(!log.info(&child.target).unwrap().closed);
    let events = log.take();
    assert_eq!(events.iter().filter(|event| matches!(event, AuxiliaryPageEvent::Closed { target } if target == &root.to_string())).count(), 1);
    assert_eq!(
        page.eval_target_expression(
            child_id,
            "document.title = 'Surviving child'; document.title",
            false,
            false
        )
        .await
        .unwrap(),
        "Surviving child"
    );
    assert!(!page.close_target(child_id).unwrap());
    stop.store(true, Ordering::Release);
    server.join().unwrap();
}
