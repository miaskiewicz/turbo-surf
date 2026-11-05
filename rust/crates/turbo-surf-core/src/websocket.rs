//! Native WebSocket client (RFC 6455 over HTTP/1.1, `ws:` and `wss:`) for the render
//! tier's `WebSocket`/`WebSocketStream`. The opening handshake is written byte for
//! byte in Chrome 156's header order and casing (measured against a localhost server,
//! harness/detection/fixtures/network-transports-chrome156.json); the response is
//! validated the way Chrome fails a connection (status, Upgrade/Connection, accept key,
//! unrequested subprotocol or extension). `permessage-deflate` is offered like Chrome
//! and honored when the server accepts it. Each connection runs on its own tokio task;
//! the owner exchanges [`WsCommand`]s and [`WsEvent`]s over channels, and dropping the
//! [`WsHandle`] tears the connection down.
pub use crate::websocket_frame::ProtocolError;
use crate::websocket_frame::{
    self as frame, Assembler, Deflater, Inbound, RawFrame, BINARY, CLOSE, PONG, TEXT,
};
use base64::Engine;
use sha1::{Digest, Sha1};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_RESPONSE_HEAD: usize = 64 * 1024;
/// How long a clean close waits for the server to drop TCP after the close frames.
const CLOSE_LINGER: std::time::Duration = std::time::Duration::from_secs(2);

/// The opening handshake inputs the embedder resolves (cookies, identity, origin).
#[derive(Clone, Debug, Default)]
pub struct WsRequest {
    /// Absolute `ws:`/`wss:` URL without a fragment.
    pub url: String,
    /// Serialized origin of the creating document.
    pub origin: String,
    pub protocols: Vec<String>,
    pub user_agent: String,
    pub accept_language: String,
    /// `Cookie` header value; omitted when empty.
    pub cookie: String,
    /// Extra DER trust anchor for `wss:` (embedder or test certificates).
    pub trust_anchor: Option<Vec<u8>>,
}

/// The server's accepted handshake.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WsAccepted {
    pub protocol: String,
    pub extensions: String,
    /// `Set-Cookie` lines of the 101 response, for the embedder's jar.
    pub set_cookies: Vec<String>,
}

/// What the connection reports, in order.
#[derive(Debug, PartialEq)]
pub enum WsEvent {
    Open(WsAccepted),
    Text(String),
    Binary(Vec<u8>),
    /// Application bytes of one message left the buffer (`bufferedAmount` drains).
    Sent(usize),
    /// The connection ended: `clean` after a closing handshake, else code 1006.
    Closed {
        clean: bool,
        code: u16,
        reason: String,
    },
    /// The connection failed (handshake or protocol error): an error then a 1006 close.
    Failed,
}

/// What the owner asks of the connection.
#[derive(Debug)]
pub enum WsCommand {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<u16>, String),
}

/// A live connection. Dropping it aborts the task and closes the socket.
pub struct WsHandle {
    commands: UnboundedSender<Outbound>,
    task: tokio::task::JoinHandle<()>,
}

impl WsHandle {
    /// Queues a command; ignored once the connection is gone.
    pub fn send(&self, command: WsCommand) {
        let outbound = match command {
            WsCommand::Text(text) => Outbound::Message(TEXT, text.into_bytes()),
            WsCommand::Binary(bytes) => Outbound::Message(BINARY, bytes),
            WsCommand::Close(code, reason) => Outbound::Close(frame::close_payload(code, &reason)),
        };
        let _ = self.commands.send(outbound);
    }
}

impl Drop for WsHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

enum Outbound {
    Message(u8, Vec<u8>),
    Pong(Vec<u8>),
    Close(Vec<u8>),
}

/// Starts connecting on the current tokio runtime; events arrive on the receiver,
/// which reports `None` once the connection task is gone.
pub fn open(request: WsRequest) -> (WsHandle, UnboundedReceiver<WsEvent>) {
    let (commands, outbound) = unbounded_channel();
    let (events_tx, events) = unbounded_channel();
    let loopback = commands.clone();
    let task = tokio::spawn(async move {
        run(request, loopback, outbound, events_tx).await;
    });
    (WsHandle { commands, task }, events)
}

async fn run(
    request: WsRequest,
    loopback: UnboundedSender<Outbound>,
    outbound: UnboundedReceiver<Outbound>,
    events: UnboundedSender<WsEvent>,
) {
    let Ok((stream, accepted, leftover)) = handshake(&request).await else {
        let _ = events.send(WsEvent::Failed);
        return;
    };
    let deflate = frame::parse_extensions(&accepted.extensions).ok().flatten();
    let _ = events.send(WsEvent::Open(accepted));
    let (read, write) = tokio::io::split(stream);
    let reader = std::io::Cursor::new(leftover).chain(read);
    let closing = Arc::new(AtomicBool::new(false));
    let session = Session {
        events: events.clone(),
        loopback,
        closing: closing.clone(),
    };
    let writer = Writer {
        deflater: deflate.and_then(Deflater::new),
        events,
        closing,
    };
    tokio::select! {
        _ = session.read(reader, Assembler::new(deflate)) => {}
        _ = writer.run(write, outbound) => {}
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut bytes)
        .expect("system randomness");
    bytes
}

/// `Sec-WebSocket-Accept` for a key.
pub fn accept_key(key: &str) -> String {
    let digest = Sha1::digest(format!("{key}{GUID}").as_bytes());
    base64::engine::general_purpose::STANDARD.encode(digest)
}

/// The request head in Chrome 156's order and casing.
pub fn handshake_head(request: &WsRequest, url: &url::Url, key: &str) -> String {
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or("")),
        None => url.host_str().unwrap_or("").to_owned(),
    };
    let path = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let mut lines = vec![
        format!("GET {path} HTTP/1.1"),
        format!("Host: {host}"),
        "Connection: Upgrade".into(),
        "Pragma: no-cache".into(),
        "Cache-Control: no-cache".into(),
        format!("User-Agent: {}", request.user_agent),
        "Upgrade: websocket".into(),
        format!("Origin: {}", request.origin),
        "Sec-WebSocket-Version: 13".into(),
        "Accept-Encoding: gzip, deflate, br, zstd".into(),
    ];
    if !request.accept_language.is_empty() {
        lines.push(format!("Accept-Language: {}", request.accept_language));
    }
    if !request.cookie.is_empty() {
        lines.push(format!("Cookie: {}", request.cookie));
    }
    lines.push(format!("Sec-WebSocket-Key: {key}"));
    lines.push("Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits".into());
    if !request.protocols.is_empty() {
        lines.push(format!(
            "Sec-WebSocket-Protocol: {}",
            request.protocols.join(", ")
        ));
    }
    format!("{}\r\n\r\n", lines.join("\r\n"))
}

async fn handshake(
    request: &WsRequest,
) -> Result<(Box<dyn Io>, WsAccepted, Vec<u8>), ProtocolError> {
    let url = url::Url::parse(&request.url).map_err(|_| ProtocolError("invalid URL"))?;
    let key = base64::engine::general_purpose::STANDARD.encode(random::<16>());
    let mut stream = connect(&url, request.trust_anchor.as_deref()).await?;
    stream
        .write_all(handshake_head(request, &url, &key).as_bytes())
        .await
        .map_err(|_| ProtocolError("write failed"))?;
    let (head, leftover) = read_head(&mut stream).await?;
    let accepted = validate_response(&head, &key, &request.protocols)?;
    Ok((stream, accepted, leftover))
}

async fn connect(url: &url::Url, anchor: Option<&[u8]>) -> Result<Box<dyn Io>, ProtocolError> {
    let host = url.host_str().ok_or(ProtocolError("missing host"))?;
    let port = url
        .port_or_known_default()
        .ok_or(ProtocolError("missing port"))?;
    let tcp = tokio::net::TcpStream::connect(format!("{host}:{port}"))
        .await
        .map_err(|_| ProtocolError("connect failed"))?;
    let _ = tcp.set_nodelay(true);
    if url.scheme() != "wss" {
        return Ok(Box::new(tcp));
    }
    let name = rustls::pki_types::ServerName::try_from(host.trim_matches(['[', ']']).to_owned())
        .map_err(|_| ProtocolError("invalid server name"))?;
    let tls = tokio_rustls::TlsConnector::from(tls_config(anchor)?)
        .connect(name, tcp)
        .await
        .map_err(|_| ProtocolError("TLS handshake failed"))?;
    Ok(Box::new(tls))
}

fn tls_config(anchor: Option<&[u8]>) -> Result<Arc<rustls::ClientConfig>, ProtocolError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(der) = anchor {
        roots
            .add(rustls::pki_types::CertificateDer::from(der.to_vec()))
            .map_err(|_| ProtocolError("invalid trust anchor"))?;
    }
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| ProtocolError("TLS configuration"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    // A WebSocket handshake is HTTP/1.1; Chrome offers only that protocol here.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

async fn read_head(stream: &mut Box<dyn Io>) -> Result<(String, Vec<u8>), ProtocolError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| ProtocolError("read failed"))?;
        if read == 0 {
            return Err(ProtocolError("connection closed during handshake"));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let leftover = buffer.split_off(end + 4);
            let head = String::from_utf8(buffer).map_err(|_| ProtocolError("invalid head"))?;
            return Ok((head, leftover));
        }
        if buffer.len() > MAX_RESPONSE_HEAD {
            return Err(ProtocolError("response head too large"));
        }
    }
}

/// Validates the 101 response the way Chrome fails a WebSocket connection.
pub fn validate_response(
    head: &str,
    key: &str,
    offered: &[String],
) -> Result<WsAccepted, ProtocolError> {
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or("");
    if status.split_whitespace().nth(1) != Some("101") {
        return Err(ProtocolError("unexpected status"));
    }
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let single = |name: &str| {
        let mut values = headers.iter().filter(|(key, _)| key == name);
        let first = values.next().map(|(_, value)| value.as_str());
        (first, values.next().is_some())
    };
    check_upgrade(single("upgrade").0, single("connection").0)?;
    if single("sec-websocket-accept") != (Some(accept_key(key).as_str()), false) {
        return Err(ProtocolError("invalid accept key"));
    }
    let protocol = check_protocol(single("sec-websocket-protocol"), offered)?;
    let extensions = single("sec-websocket-extensions").0.unwrap_or("");
    frame::parse_extensions(extensions)?;
    Ok(WsAccepted {
        protocol,
        extensions: extensions.to_owned(),
        set_cookies: headers
            .iter()
            .filter(|(name, _)| name == "set-cookie")
            .map(|(_, value)| value.clone())
            .collect(),
    })
}

fn check_upgrade(upgrade: Option<&str>, connection: Option<&str>) -> Result<(), ProtocolError> {
    let upgrade_ok = upgrade.is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let connection_ok = connection.is_some_and(|value| {
        value
            .split(',')
            .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
    });
    if upgrade_ok && connection_ok {
        Ok(())
    } else {
        Err(ProtocolError("missing upgrade"))
    }
}

fn check_protocol(
    (value, repeated): (Option<&str>, bool),
    offered: &[String],
) -> Result<String, ProtocolError> {
    match value {
        None => Ok(String::new()),
        Some(value) if !repeated && offered.iter().any(|protocol| protocol == value) => {
            Ok(value.to_owned())
        }
        Some(_) => Err(ProtocolError("unrequested subprotocol")),
    }
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<RawFrame> {
    let mut head = [0u8; 2];
    reader.read_exact(&mut head).await?;
    let (extended, masked) = frame::header_layout(head);
    let mut length = [0u8; 8];
    reader.read_exact(&mut length[..extended]).await?;
    let size = frame::payload_length(head, &length[..extended])
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.0))?;
    let mut mask = [0u8; 4];
    if masked {
        reader.read_exact(&mut mask).await?;
    }
    let mut payload = vec![0; size];
    reader.read_exact(&mut payload).await?;
    Ok(frame::raw_frame(head, masked, payload))
}

/// The read side: reports units and answers pings and close frames.
struct Session {
    events: UnboundedSender<WsEvent>,
    loopback: UnboundedSender<Outbound>,
    closing: Arc<AtomicBool>,
}

impl Session {
    async fn read<R: AsyncRead + Unpin>(self, mut reader: R, mut assembler: Assembler) {
        loop {
            let unit = match read_frame(&mut reader).await {
                Ok(raw) => assembler.accept(raw),
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                    Err(ProtocolError("invalid frame"))
                }
                Err(_) => return self.abnormal(),
            };
            match unit {
                Ok(Some(Inbound::Close(code, reason))) => {
                    return self.closed(&mut reader, code, reason).await
                }
                Ok(Some(unit)) => self.deliver(unit),
                Ok(None) => {}
                Err(_) => {
                    let _ = self.events.send(WsEvent::Failed);
                    return;
                }
            }
        }
    }

    fn deliver(&self, unit: Inbound) {
        let event = match unit {
            Inbound::Text(text) => WsEvent::Text(text),
            Inbound::Binary(bytes) => WsEvent::Binary(bytes),
            Inbound::Ping(payload) => {
                let _ = self.loopback.send(Outbound::Pong(payload));
                return;
            }
            _ => return,
        };
        let _ = self.events.send(event);
    }

    fn abnormal(&self) {
        let _ = self.events.send(WsEvent::Closed {
            clean: false,
            code: 1006,
            reason: String::new(),
        });
    }

    /// The server's close frame: echo it unless ours went first, then let the server
    /// drop TCP (bounded) before reporting the clean closure with the server's code.
    async fn closed<R: AsyncRead + Unpin>(
        &self,
        reader: &mut R,
        code: Option<u16>,
        reason: String,
    ) {
        if !self.closing.load(Ordering::SeqCst) {
            let _ = self
                .loopback
                .send(Outbound::Close(frame::close_payload(code, "")));
        }
        let mut sink = [0u8; 512];
        let drain = async { while matches!(reader.read(&mut sink).await, Ok(1..)) {} };
        let _ = tokio::time::timeout(CLOSE_LINGER, drain).await;
        let _ = self.events.send(WsEvent::Closed {
            clean: true,
            code: code.unwrap_or(1005),
            reason,
        });
    }
}

/// The write side: frames, masks and (when negotiated) compresses outbound messages.
struct Writer {
    deflater: Option<Deflater>,
    events: UnboundedSender<WsEvent>,
    closing: Arc<AtomicBool>,
}

impl Writer {
    async fn run<W: AsyncWrite + Unpin>(
        mut self,
        mut write: W,
        mut outbound: UnboundedReceiver<Outbound>,
    ) {
        while let Some(item) = outbound.recv().await {
            if self.closing.load(Ordering::SeqCst) {
                continue;
            }
            let (bytes, sent) = self.encode(item);
            if write.write_all(&bytes).await.is_err() {
                // The read side reports the closure; keep the task alive until it does.
                std::future::pending::<()>().await;
            }
            if let Some(sent) = sent {
                let _ = self.events.send(WsEvent::Sent(sent));
            }
        }
    }

    fn encode(&mut self, item: Outbound) -> (Vec<u8>, Option<usize>) {
        let mask = random::<4>();
        match item {
            Outbound::Message(opcode, payload) => {
                let size = payload.len();
                let frame = match self.deflater.as_mut() {
                    Some(deflater) => {
                        frame::encode(opcode, &deflater.deflate(&payload), true, mask)
                    }
                    None => frame::encode(opcode, &payload, false, mask),
                };
                (frame, Some(size))
            }
            Outbound::Pong(payload) => (frame::encode(PONG, &payload, false, mask), None),
            Outbound::Close(payload) => {
                self.closing.store(true, Ordering::SeqCst);
                (frame::encode(CLOSE, &payload, false, mask), None)
            }
        }
    }
}
