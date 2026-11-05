//! The Rust twin of harness/detection/network-transports-server.mjs: the same routes,
//! WebSocket commands and event streams, on its own thread and runtime, with handshake
//! and EventSource request headers logged in wire order and the same masking.
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

type Headers = Vec<(String, String)>;
#[derive(Default)]
struct Log {
    ws: Vec<serde_json::Value>,
    sse: Vec<serde_json::Value>,
}

pub struct TransportServer {
    pub port: u16,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for TransportServer {
    fn drop(&mut self) {
        let _ = self.stop.take().map(|stop| stop.send(()));
        let _ = self.thread.take().map(|thread| thread.join());
    }
}

impl TransportServer {
    pub fn start(pages: &'static [(&'static str, &'static str)]) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let log = Arc::new(Mutex::new(Log::default()));
                let accept = async {
                    loop {
                        let (socket, _) = listener.accept().await.unwrap();
                        tokio::spawn(connection(socket, port, pages, log.clone()));
                    }
                };
                tokio::select! {
                    _ = accept => {}
                    _ = stopped => {}
                }
            });
        });
        Self {
            port,
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}

fn mask(headers: &Headers, port: u16) -> serde_json::Value {
    use base64::Engine;
    let rows: Vec<_> = headers
        .iter()
        .map(|(name, value)| {
            let value = match name.to_ascii_lowercase().as_str() {
                "sec-websocket-key" => {
                    let decoded = base64::engine::general_purpose::STANDARD.decode(value);
                    if decoded.is_ok_and(|bytes| bytes.len() == 16) {
                        "<16-byte base64>".to_owned()
                    } else {
                        format!("<invalid {value}>")
                    }
                }
                "user-agent" => "<user-agent>".to_owned(),
                _ => value.replace(&format!(":{port}"), ":<port>"),
            };
            serde_json::json!([name, value])
        })
        .collect();
    serde_json::Value::Array(rows)
}

async fn read_head(socket: &mut TcpStream) -> Option<(String, Headers)> {
    let mut head = Vec::new();
    let mut byte = [0u8];
    while !head.ends_with(b"\r\n\r\n") {
        if socket.read(&mut byte).await.ok()? == 0 {
            return None;
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let target = lines.next()?.split_whitespace().nth(1)?.to_owned();
    let headers = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    Some((target, headers))
}

fn header<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

async fn connection(
    mut socket: TcpStream,
    port: u16,
    pages: &'static [(&'static str, &'static str)],
    log: Arc<Mutex<Log>>,
) {
    let Some((target, headers)) = read_head(&mut socket).await else {
        return;
    };
    let path = target.split('?').next().unwrap_or("").to_owned();
    if header(&headers, "upgrade").is_some() {
        log.lock()
            .unwrap()
            .ws
            .push(serde_json::json!({"path": target, "headers": mask(&headers, port)}));
        return upgrade(socket, &target, &headers).await;
    }
    if path.starts_with("/sse/") {
        let attempt = {
            let mut log = log.lock().unwrap();
            log.sse
                .push(serde_json::json!({"path": path, "headers": mask(&headers, port)}));
            log.sse
                .iter()
                .filter(|entry| entry["path"] == path.as_str())
                .count()
        };
        return event_stream(socket, &path, attempt).await;
    }
    let (kind, body) = match path.as_str() {
        "/log/ws" => (
            "application/json",
            serde_json::to_string(&log.lock().unwrap().ws).unwrap(),
        ),
        "/log/sse" => (
            "application/json",
            serde_json::to_string(&log.lock().unwrap().sse).unwrap(),
        ),
        _ => {
            let page = pages
                .iter()
                .find(|(key, _)| *key == path)
                .unwrap_or(&pages[0]);
            ("text/html", page.1.to_owned())
        }
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

async fn event_stream(mut socket: TcpStream, path: &str, attempt: usize) {
    const STREAM: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    let reply = match (path, attempt) {
        ("/sse/stream", 1) => format!("{STREAM}\u{feff}retry: 30\nid: 7\ndata: one\n\nevent: custom\ndata: two\ndata: three\n\n: comment\n\ndata: four\r\n\r\nid\ndata: five\n\nid: 9\ndata: six\n\ndata: unterminated"),
        ("/sse/stream", 2) => format!("{STREAM}data: again\r\rid: 10\n\n"),
        ("/sse/stream", _) => "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_owned(),
        ("/sse/wrong-type", _) => "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\ndata: no\n\n".to_owned(),
        ("/sse/forever", _) => format!("{STREAM}data: first\n\n"),
        _ => "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 7\r\nConnection: close\r\n\r\nmissing".to_owned(),
    };
    let _ = socket.write_all(reply.as_bytes()).await;
    if path == "/sse/forever" {
        // Held open until the client goes away.
        let mut sink = [0u8; 64];
        while matches!(socket.read(&mut sink).await, Ok(1..)) {}
    }
}

async fn upgrade(mut socket: TcpStream, target: &str, headers: &Headers) {
    if target.starts_with("/ws/reject") {
        let _ = socket
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            .await;
        return;
    }
    let key = header(headers, "sec-websocket-key").unwrap_or("");
    let accept = if target.starts_with("/ws/bad-accept") {
        "AAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_owned()
    } else {
        turbo_surf_core::websocket::accept_key(key)
    };
    let mut lines = vec![
        "HTTP/1.1 101 Switching Protocols".to_owned(),
        "Upgrade: websocket".into(),
        "Connection: Upgrade".into(),
        format!("Sec-WebSocket-Accept: {accept}"),
    ];
    let offered = header(headers, "sec-websocket-protocol").unwrap_or("");
    if target.contains("proto=bogus") {
        lines.push("Sec-WebSocket-Protocol: bogus".into());
    } else if offered.split(',').any(|part| part.trim() == "chat") {
        lines.push("Sec-WebSocket-Protocol: chat".into());
    }
    let deflate = target.contains("deflate");
    if deflate {
        lines.push("Sec-WebSocket-Extensions: permessage-deflate; server_no_context_takeover; client_no_context_takeover".into());
    }
    let _ = socket
        .write_all(format!("{}\r\n\r\n", lines.join("\r\n")).as_bytes())
        .await;
    Peer {
        socket,
        deflate,
        close_sent: false,
    }
    .serve()
    .await;
}

fn frame(opcode: u8, payload: &[u8], fin: bool, rsv1: bool) -> Vec<u8> {
    let mut out = vec![if fin { 0x80 } else { 0 } | if rsv1 { 0x40 } else { 0 } | opcode];
    let length = payload.len();
    if length < 126 {
        out.push(length as u8);
    } else if length < 65536 {
        out.push(126);
        out.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(length as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

fn deflate_raw(input: &[u8]) -> Vec<u8> {
    let mut compress = Compress::new(Compression::default(), false);
    let mut out = Vec::with_capacity(input.len() + 64);
    compress
        .compress_vec(input, &mut out, FlushCompress::Sync)
        .unwrap();
    out.truncate(out.len() - 4);
    out
}

fn inflate_raw(mut input: Vec<u8>) -> Vec<u8> {
    input.extend_from_slice(&[0, 0, 0xff, 0xff]);
    let mut decompress = Decompress::new(false);
    let mut out = Vec::with_capacity(input.len() * 8 + 1024);
    decompress
        .decompress_vec(&input, &mut out, FlushDecompress::Sync)
        .unwrap();
    out
}

struct Peer {
    socket: TcpStream,
    deflate: bool,
    close_sent: bool,
}

struct ClientFrame {
    fin: bool,
    rsv1: bool,
    opcode: u8,
    masked: bool,
    payload: Vec<u8>,
}

impl Peer {
    async fn read_frame(&mut self) -> Option<ClientFrame> {
        let mut head = [0u8; 2];
        self.socket.read_exact(&mut head).await.ok()?;
        let mut length = u64::from(head[1] & 0x7f);
        if length == 126 {
            let mut bytes = [0u8; 2];
            self.socket.read_exact(&mut bytes).await.ok()?;
            length = u64::from(u16::from_be_bytes(bytes));
        } else if length == 127 {
            let mut bytes = [0u8; 8];
            self.socket.read_exact(&mut bytes).await.ok()?;
            length = u64::from_be_bytes(bytes);
        }
        let masked = head[1] & 0x80 != 0;
        let mut mask = [0u8; 4];
        if masked {
            self.socket.read_exact(&mut mask).await.ok()?;
        }
        let mut payload = vec![0u8; length as usize];
        self.socket.read_exact(&mut payload).await.ok()?;
        for (at, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[at & 3];
        }
        Some(ClientFrame {
            fin: head[0] & 0x80 != 0,
            rsv1: head[0] & 0x40 != 0,
            opcode: head[0] & 0x0f,
            masked,
            payload,
        })
    }

    async fn write(&mut self, bytes: &[u8]) {
        let _ = self.socket.write_all(bytes).await;
    }

    async fn send(&mut self, opcode: u8, payload: &[u8]) {
        let bytes = if self.deflate {
            frame(opcode, &deflate_raw(payload), true, true)
        } else {
            frame(opcode, payload, true, false)
        };
        self.write(&bytes).await;
    }

    async fn serve(mut self) {
        let mut message: Option<(u8, bool, Vec<u8>)> = None;
        while let Some(item) = self.read_frame().await {
            if !item.masked {
                let mut payload = 1002u16.to_be_bytes().to_vec();
                payload.extend_from_slice(b"unmasked");
                return self.write(&frame(8, &payload, true, false)).await;
            }
            if item.opcode >= 8 {
                if !self.control(item).await {
                    return;
                }
                continue;
            }
            let entry = message.get_or_insert((item.opcode, item.rsv1, Vec::new()));
            entry.2.extend_from_slice(&item.payload);
            if item.fin {
                let (opcode, compressed, payload) = message.take().unwrap();
                let payload = if compressed {
                    inflate_raw(payload)
                } else {
                    payload
                };
                if !self.deliver(opcode, payload).await {
                    return;
                }
            }
        }
    }

    async fn control(&mut self, item: ClientFrame) -> bool {
        match item.opcode {
            8 => {
                if !self.close_sent {
                    self.write(&frame(8, &item.payload, true, false)).await;
                }
                let _ = self.socket.shutdown().await;
                false
            }
            10 => {
                let mut text = b"pong:".to_vec();
                text.extend_from_slice(&item.payload);
                self.send(1, &text).await;
                true
            }
            _ => true,
        }
    }

    async fn deliver(&mut self, opcode: u8, payload: Vec<u8>) -> bool {
        if opcode != 1 {
            self.send(2, &payload).await;
            return true;
        }
        let text = String::from_utf8(payload.clone()).unwrap();
        if let Some(rest) = text.strip_prefix("close:") {
            let (code, reason) = rest.split_once(':').unwrap();
            let mut close = code.parse::<u16>().unwrap().to_be_bytes().to_vec();
            close.extend_from_slice(reason.as_bytes());
            self.close_sent = true;
            self.write(&frame(8, &close, true, false)).await;
            return true;
        }
        match text.as_str() {
            "binary" => self.send(2, &[9, 8, 7]).await,
            "frag" => {
                self.write(&frame(1, b"ab", false, false)).await;
                self.write(&frame(0, b"cd", true, false)).await;
            }
            "ping" => self.write(&frame(9, b"p1", true, false)).await,
            "big" => self.send(1, &vec![b'x'; 70000]).await,
            "drop" => return false,
            "badutf8" => self.write(&frame(1, &[0xff, 0xfe], true, false)).await,
            _ => self.send(1, &payload).await,
        }
        true
    }
}
