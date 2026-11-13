use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::{AuxiliaryCapability, AuxiliaryHostServices, PageSession};

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub struct Server {
    pub origin: String,
    requests: Arc<Mutex<Vec<Request>>>,
    pub closed: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(tokio::sync::Notify::new());
        let log = requests.clone();
        let close = closed.clone();
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            while let Ok((socket, _)) = listener.accept().await {
                let log = log.clone();
                let close = close.clone();
                tasks.spawn(async move {
                    serve(socket, log, close).await;
                });
            }
        });
        Self {
            origin,
            requests,
            closed,
            task,
        }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Request {
    let mut bytes = Vec::new();
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
        let mut chunk = [0; 1024];
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0, "request closed before its headers");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() < 65_536, "fixture request headers too large");
    };
    let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let mut start = head.lines().next().unwrap().split_whitespace();
    let method = start.next().unwrap().to_owned();
    let target = start.next().unwrap().to_owned();
    let headers: BTreeMap<String, String> = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect();
    let length: usize = headers
        .get("content-length")
        .map_or(0, |value| value.parse().unwrap());
    while bytes.len() < end + length {
        let mut chunk = [0; 1024];
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0, "request closed before its body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    Request {
        method,
        target,
        headers,
        body: bytes[end..end + length].to_vec(),
    }
}

async fn serve(
    mut socket: tokio::net::TcpStream,
    log: Arc<Mutex<Vec<Request>>>,
    closed: Arc<tokio::sync::Notify>,
) {
    let request = read_request(&mut socket).await;
    let path = request.target.clone();
    log.lock().unwrap().push(request);
    if path == "/hold" {
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut byte = [0];
        match socket.read(&mut byte).await {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            result => panic!("held navigation was not cancelled: {result:?}"),
        }
        closed.notify_one();
        return;
    }
    let (status, headers, body) = response(&path);
    let head = format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await.unwrap();
    socket.write_all(&body).await.unwrap();
}

fn response(path: &str) -> (&'static str, &'static str, Vec<u8>) {
    match path {
        "/redirect" => ("303 See Other", "Location: /one\r\n", Vec::new()),
        "/attachment" => ("200 Fixture", "Content-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=owned.bin\r\n", vec![0, 255, 65, 10]),
        "/coop" => ("200 Fixture", "Content-Type: text/html\r\nCross-Origin-Opener-Policy: same-origin\r\n", b"<!doctype html><title>isolated</title>".to_vec()),
        _ => {
            let title = path.trim_start_matches('/');
            let body = format!("<!doctype html><title>{title}</title><script>globalThis.bootObservation=[globalThis.contextValue??null,globalThis.pageOnly??null]</script><p id=content>{title}</p>");
            ("200 Fixture", "Content-Type: text/html\r\n", body.into_bytes())
        }
    }
}

pub async fn open(origin: &str, services: AuxiliaryHostServices) -> PageSession {
    let page = PageSession::open_with_document_response_and_auxiliary_services(
        "<!doctype html><title>opener</title>",
        &format!("{origin}/opener"),
        "",
        "",
        5_000,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        Default::default(),
        "",
        None,
        services,
    )
    .await
    .unwrap();
    assert!(matches!(
        page.auxiliary_event_log().capability(),
        AuxiliaryCapability::Supported
    ));
    page
}

pub async fn child(page: &mut PageSession) -> u64 {
    let root = page.auxiliary_root_id().unwrap().to_string();
    page.eval(
        "globalThis.child=window.open('', 'contract-child');child.document.title='original';",
    )
    .await
    .unwrap();
    page.auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.source_page.as_deref() == Some(&root) && !info.closed)
        .unwrap()
        .target
        .parse()
        .unwrap()
}

pub async fn settle(page: &mut PageSession) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while page.has_pending_auxiliary_work() {
            page.poll_owned_work().await.unwrap();
        }
    })
    .await
    .expect("owned popup lifecycle must settle");
}
