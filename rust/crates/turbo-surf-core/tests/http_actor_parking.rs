use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use turbo_surf_core::{
    net::{fetch_bytes, FetchOptions},
    request_context::ClientHints,
};

#[tokio::test(flavor = "current_thread")]
async fn empty_posts_after_actor_parking_are_delivered_once_without_replay() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let writes = Arc::new(AtomicUsize::new(0));
    let stopped = stop.clone();
    let observed = writes.clone();
    let server = std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(10);
        let mut children = Vec::new();
        while !stopped.load(Ordering::Relaxed) && Instant::now() < end {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket.set_nonblocking(false).unwrap();
                    let observed = observed.clone();
                    children.push(std::thread::spawn(move||{
                    socket.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
                    loop {
                        let mut bytes = Vec::new();
                        while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                            let mut chunk = [0; 4096];
                            let n = socket.read(&mut chunk).unwrap_or(0);
                            if n == 0 { return; }
                            bytes.extend_from_slice(&chunk[..n]);
                        }
                        let status = if bytes.starts_with(b"GET ") { "200 OK" } else {
                            assert!(bytes.starts_with(b"POST "));
                            observed.fetch_add(1, Ordering::Relaxed);
                            "201 Created"
                        };
                        let response = format!("HTTP/1.1 {status}\r\nContent-Length: 2\r\nContent-Type: text/plain\r\nConnection: keep-alive\r\n\r\nok");
                        if socket.write_all(response.as_bytes()).is_err() { return; }
                    }
                }));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("{error}"),
            }
        }
        for child in children {
            child.join().unwrap();
        }
    });
    let hints = ClientHints::default();
    let mut failures = Vec::new();
    for index in 0..5 {
        fetch_bytes(
            &format!("{origin}/warm/{index}"),
            FetchOptions {
                client_hints: Some(&hints),
                allow_non_html: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        // Emulate a synchronous guest turn while the peer expires its idle socket.
        std::thread::sleep(Duration::from_millis(150));
        let result = fetch_bytes(
            &format!("{origin}/revoke/{index}"),
            FetchOptions {
                client_hints: Some(&hints),
                method: Some("POST".into()),
                allow_non_html: true,
                ..Default::default()
            },
        )
        .await;
        if let Err(error) = result {
            failures.push(error.to_string());
        }
    }
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(writes.load(Ordering::Relaxed), 5);
}
