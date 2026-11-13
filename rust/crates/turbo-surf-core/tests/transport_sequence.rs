use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Barrier;
use tokio::task::JoinHandle;
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_core::net::{fetch_html, ErrorCode, FetchOptions};
use turbo_surf_core::request_context::{ClientHints, Destination, RequestContext, RequestMode};

#[derive(Clone, Debug)]
struct Request {
    connection: usize,
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Server {
    origin: String,
    requests: Arc<Mutex<Vec<Request>>>,
    task: JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn start(reset_password_post: bool) -> Self {
        Self::configured(reset_password_post, None).await
    }

    async fn configured(
        reset_password_post: bool,
        idle_reset: Option<std::sync::mpsc::Sender<usize>>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let chunks = Arc::new(Barrier::new(5));
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            let mut next = 0;
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        next += 1;
                        connections.spawn(serve(socket, next, log.clone(), chunks.clone(), reset_password_post, idle_reset.clone()));
                    }
                    Some(result) = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap();
                    }
                }
            }
        });
        Self {
            origin,
            requests,
            task,
        }
    }
}

async fn read_request(
    socket: &mut TcpStream,
    buffered: &mut Vec<u8>,
    connection: usize,
) -> Option<Request> {
    let end = loop {
        if let Some(end) = buffered.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break end + 4;
        }
        let mut chunk = [0; 4096];
        let count = socket.read(&mut chunk).await.unwrap();
        if count == 0 {
            return None;
        }
        buffered.extend_from_slice(&chunk[..count]);
        assert!(
            buffered.len() <= 128 * 1024,
            "fixture header exceeded its bound"
        );
    };
    let head = std::str::from_utf8(&buffered[..end]).unwrap();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap().split_whitespace();
    let method = first.next().unwrap().to_string();
    let path = first.next().unwrap().to_string();
    assert_eq!(first.next(), Some("HTTP/1.1"));
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    assert!(
        !headers.contains_key("transfer-encoding"),
        "fixture requires a fixed password body"
    );
    let length: usize = headers
        .get("content-length")
        .map_or(0, |value| value.parse().unwrap());
    assert!(length < 64 * 1024);
    while buffered.len() < end + length {
        let mut chunk = [0; 4096];
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0, "request body ended early");
        buffered.extend_from_slice(&chunk[..count]);
    }
    let body = buffered[end..end + length].to_vec();
    buffered.drain(..end + length);
    Some(Request {
        connection,
        method,
        path,
        headers,
        body,
    })
}

async fn serve(
    mut socket: TcpStream,
    connection: usize,
    log: Arc<Mutex<Vec<Request>>>,
    chunks: Arc<Barrier>,
    reset_password_post: bool,
    idle_reset: Option<std::sync::mpsc::Sender<usize>>,
) {
    let mut buffered = Vec::new();
    loop {
        let request = if let Some(closed) = &idle_reset {
            match tokio::time::timeout(
                Duration::from_millis(50),
                read_request(&mut socket, &mut buffered, connection),
            )
            .await
            {
                Ok(request) => request,
                Err(_) => {
                    socket.set_zero_linger().unwrap();
                    drop(socket);
                    let _ = closed.send(connection);
                    return;
                }
            }
        } else {
            read_request(&mut socket, &mut buffered, connection).await
        };
        let Some(request) = request else {
            return;
        };
        let attempt = {
            let mut log = log.lock().unwrap();
            let attempt = log.iter().filter(|seen| seen.path == request.path).count() + 1;
            log.push(request.clone());
            attempt
        };
        if (reset_password_post && request.path == "/api/v1/login")
            || (request.path == "/reset-once" && attempt == 1)
            || request.path == "/reset-always"
            || request.path == "/reset-body"
        {
            // Deliberate RST after consuming the entire non-idempotent request.
            socket.set_zero_linger().unwrap();
            return;
        }
        if request.path == "/invalid-response" {
            socket
                .write_all(b"not an HTTP response\r\n\r\n")
                .await
                .unwrap();
            return;
        }
        let (status, headers, body) = match request.path.as_str() {
            "/api/v1/login" => ("303 See Other", "Location: /post-login\r\nSet-Cookie: session=fixture; Path=/; HttpOnly; SameSite=Lax\r\n", String::new()),
            path if path.starts_with("/_next/static/chunks/") => {
                if idle_reset.is_none() { chunks.wait().await; }
                ("200 OK", "Content-Type: application/javascript\r\n", format!("globalThis.loaded={path:?};"))
            }
            _ => ("200 OK", "Content-Type: text/html\r\n", "<body>fixture</body>".to_string()),
        };
        let wire_body = if request.method == "HEAD" { "" } else { &body };
        let response = format!("HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: keep-alive\r\n\r\n{wire_body}", body.len());
        if socket.write_all(response.as_bytes()).await.is_err() {
            return;
        }
    }
}

async fn sign_in(
    server: &Server,
    hints: &ClientHints,
    jar: &mut CookieJar,
) -> Result<(), turbo_surf_core::net::HttpError> {
    for path in ["/login", "/api/v1/refresh_token"] {
        fetch_html(
            &format!("{}{path}", server.origin),
            FetchOptions {
                client_hints: Some(hints),
                jar: Some(&mut *jar),
                ..Default::default()
            },
        )
        .await?;
    }
    let response = fetch_html(
        &format!("{}/api/v1/login", server.origin),
        FetchOptions {
            client_hints: Some(hints),
            jar: Some(jar),
            context: Some(RequestContext::resource(
                &server.origin,
                Destination::Empty,
                RequestMode::Cors,
            )),
            method: Some("POST".into()),
            body: Some("password=disposable-fixture-only".into()),
            headers: BTreeMap::from([(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            )]),
            ..Default::default()
        },
    )
    .await?;
    assert!(response.redirected);
    assert_eq!(response.final_url, format!("{}/post-login", server.origin));
    Ok(())
}

#[tokio::test]
async fn pooled_http1_login_redirect_preserves_five_parallel_script_requests() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let server = Server::start(false).await;
        let hints = ClientHints::default();
        let mut jar = CookieJar::new();
        sign_in(&server, &hints, &mut jar).await.unwrap();
        let requests = (0..5).map(|index| {
            let hints = hints.clone();
            let mut jar = jar.fork_for_request();
            let origin = server.origin.clone();
            async move {
                let path = format!("/_next/static/chunks/fixture-{index}.js");
                let response = fetch_html(
                    &format!("{origin}{path}"),
                    FetchOptions {
                        client_hints: Some(&hints),
                        jar: Some(&mut jar),
                        allow_non_html: true,
                        context: Some(RequestContext::resource(
                            &origin,
                            Destination::Script,
                            RequestMode::Cors,
                        )),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                assert_eq!(response.status, 200);
                assert!(response.html.contains(&path));
            }
        });
        futures_util::future::join_all(requests).await;
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 9);
        assert!(requests[..4]
            .iter()
            .all(|request| request.connection == requests[0].connection));
        assert_eq!(requests[2].method, "POST");
        assert_eq!(requests[2].body, b"password=disposable-fixture-only");
        assert_eq!(requests[3].method, "GET");
        assert!(requests[3].body.is_empty());
        assert!(requests[4..]
            .iter()
            .any(|request| request.connection == requests[0].connection));
        for request in &requests[4..] {
            assert_eq!(
                request.headers.get("cookie").map(String::as_str),
                Some("session=fixture")
            );
            assert_eq!(
                request.headers.get("sec-fetch-dest").map(String::as_str),
                Some("script")
            );
        }
    })
    .await
    .expect("bounded localhost login/chunk sequence");
}

#[tokio::test]
async fn a_consumed_password_post_reset_surfaces_a_network_error() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let server = Server::start(true).await;
        let error = sign_in(&server, &ClientHints::default(), &mut CookieJar::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Network);
        let requests = server.requests.lock().unwrap();
        let posts: Vec<_> = requests
            .iter()
            .filter(|request| request.path == "/api/v1/login")
            .collect();
        assert_eq!(
            posts.len(),
            1,
            "a consumed password POST must not be replayed"
        );
        assert_eq!(posts[0].body, b"password=disposable-fixture-only");
    })
    .await
    .expect("bounded consumed-request reset control");
}

#[test]
fn same_current_thread_runtime_recovers_idle_pool_after_host_parking() {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (closed_tx, closed_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let server = Server::configured(false, Some(closed_tx)).await;
            ready_tx
                .send((server.origin.clone(), server.requests.clone()))
                .unwrap();
            let _ = stop_rx.await;
            drop(server);
            tokio::task::yield_now().await;
        });
    });
    let (origin, log) = ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let hints = ClientHints::default();
    runtime.block_on(async {
        fetch_html(
            &format!("{origin}/login"),
            FetchOptions {
                client_hints: Some(&hints),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    });
    // The server's independent runtime expires the connection while this same
    // client runtime is parked between host calls, as in synchronous NAPI calls.
    let closed = closed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(closed, log.lock().unwrap()[0].connection);
    let results = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(5),
            futures_util::future::join_all((0..5).map(|index| {
                let hints = hints.clone();
                let origin = origin.clone();
                async move {
                    fetch_html(
                        &format!("{origin}/_next/static/chunks/parked-{index}.js"),
                        FetchOptions {
                            client_hints: Some(&hints),
                            allow_non_html: true,
                            context: Some(RequestContext::resource(
                                &origin,
                                Destination::Script,
                                RequestMode::Cors,
                            )),
                            ..Default::default()
                        },
                    )
                    .await
                }
            })),
        )
        .await
    });
    let _ = stop_tx.send(());
    server.join().unwrap();
    let results = results.expect("bounded resumed parallel requests");
    eprintln!(
        "parked results: {:?}; parsed paths: {:?}",
        results
            .iter()
            .map(|result| result
                .as_ref()
                .map(|response| response.status)
                .map_err(|error| error.message.as_str()))
            .collect::<Vec<_>>(),
        log.lock()
            .unwrap()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>()
    );
    for (index, result) in results.into_iter().enumerate() {
        assert_eq!(
            result
                .unwrap_or_else(|error| panic!("chunk {index}: {error:?}"))
                .status,
            200
        );
    }
}

#[tokio::test]
async fn consumed_get_and_head_reset_recover_once_without_replacing_session_transport() {
    for method in ["GET", "HEAD"] {
        let server = Server::start(false).await;
        let hints = ClientHints::with_client_builder(|| {
            let mut headers = turbo_surf_core::http_backend::header::HeaderMap::new();
            headers.insert(
                "x-fixture-client",
                turbo_surf_core::http_backend::header::HeaderValue::from_static("configured"),
            );
            Ok(turbo_surf_core::net::client_builder().default_headers(headers))
        });
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            fetch_html(
                &format!("{}/reset-once", server.origin),
                FetchOptions {
                    method: Some(method.into()),
                    client_hints: Some(&hints),
                    context: Some(RequestContext::resource(
                        &server.origin,
                        Destination::Script,
                        RequestMode::Cors,
                    )),
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            result
                .unwrap_or_else(|error| panic!("{method}: {error:?}"))
                .status,
            200
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "{method}: {requests:?}");
        assert!(requests.iter().all(|request| request
            .headers
            .get("x-fixture-client")
            .map(String::as_str)
            == Some("configured")));
        assert!(requests
            .iter()
            .all(|request| request.method == method && request.body.is_empty()));
    }
}

#[tokio::test]
async fn repeated_reset_get_is_bounded_to_two_attempts() {
    let server = Server::start(false).await;
    let hints = ClientHints::default();
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        fetch_html(
            &format!("{}/reset-always", server.origin),
            FetchOptions {
                client_hints: Some(&hints),
                ..Default::default()
            },
        ),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Network);
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn get_with_a_body_and_non_reset_protocol_errors_are_not_retried() {
    for (path, body) in [
        (
            "/reset-body",
            Some("nonreplayable fixture body".to_string()),
        ),
        ("/reset-body", Some(String::new())),
        ("/invalid-response", None),
    ] {
        let server = Server::start(false).await;
        let hints = ClientHints::default();
        let error = tokio::time::timeout(
            Duration::from_secs(3),
            fetch_html(
                &format!("{}{path}", server.origin),
                FetchOptions {
                    method: Some("GET".into()),
                    body,
                    client_hints: Some(&hints),
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Network);
        assert_eq!(server.requests.lock().unwrap().len(), 1, "{path}");
    }
}
