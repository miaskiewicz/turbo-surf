//! A failed public operation must not start queued form navigation afterward.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use turbo_surf_render::PageSession;

struct Server {
    origin: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let count = requests.clone();
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => respond(stream, &count),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::park_timeout(std::time::Duration::from_millis(1))
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        });
        Self {
            origin,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}
fn respond(mut stream: TcpStream, count: &AtomicUsize) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    let mut request = Vec::new();
    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
        let mut chunk = [0; 4096];
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            return;
        }
        request.extend_from_slice(&chunk[..read]);
        assert!(request.len() <= 16384);
    }
    assert!(request.starts_with(b"GET /landed?actual=submitted HTTP/1.1\r\n"));
    count.fetch_add(1, Ordering::Release);
    let body = "<!doctype html><p id='landed'>Landed</p>";
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes());
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            thread.join().unwrap();
        }
    }
}
async fn page(server: &Server) -> PageSession {
    PageSession::open(
        "<!doctype html><form action='/landed'><input name='actual' value='submitted'></form>",
        &format!("{}/start", server.origin),
        "",
        "",
        100,
    )
    .await
    .unwrap()
}

async fn interrupted(expression: bool) {
    let server = Server::start();
    let mut page = page(&server).await;
    assert_eq!(server.requests.load(Ordering::Acquire), 0);
    let result = if expression {
        page.eval_expression("new Promise(()=>setTimeout(()=>{document.querySelector('form').submit();while(true){}},0))").await
    } else {
        page.eval("setTimeout(()=>{document.querySelector('form').submit();while(true){}},0)")
            .await
    };
    let requests = server.requests.load(Ordering::Acquire);
    eprintln!("expression={expression}; result={result:?}; actualHTTPrequests={requests}");
    page.close();
    assert_eq!(
        requests, 0,
        "the public wrapper must not follow a queued form after hard interruption"
    );
    assert_eq!(
        result.unwrap_err(),
        "Execution interrupted by the session execution budget"
    );
}
#[tokio::test(flavor = "current_thread")]
async fn public_eval_does_not_follow_a_form_after_hard_interruption() {
    interrupted(false).await
}
#[tokio::test(flavor = "current_thread")]
async fn public_owned_expression_does_not_follow_a_form_after_hard_interruption() {
    interrupted(true).await
}
#[tokio::test(flavor = "current_thread")]
async fn an_ordinary_public_form_submission_still_installs_its_response() {
    let server = Server::start();
    let mut page = page(&server).await;
    page.eval("document.querySelector('form').submit()")
        .await
        .unwrap();
    assert_eq!(server.requests.load(Ordering::Acquire), 1);
    assert_eq!(
        page.eval_expression("document.getElementById('landed')!==null")
            .await
            .unwrap(),
        "true"
    );
    page.close();
}
