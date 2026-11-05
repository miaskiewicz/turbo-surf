//! turbo-surf#179: a `wss:` connection's ClientHello is surf's Chrome https ClientHello
//! with only Chrome's own WebSocket differences (HTTP/1.1-only ALPN, so no ALPS). The
//! Chrome side is the frozen 156 control recorded by harness/detection/wss-hello.mjs.
//! Localhost listeners close after the ClientHello; no certificate exchange, no HTTP.
#![cfg(feature = "impersonate")]

use std::collections::BTreeSet;
use tokio::io::AsyncReadExt;
use turbo_surf_core::net::{fetch_html, FetchOptions};
use turbo_surf_core::websocket::{open, WsRequest};

const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/wss-hello-chrome156.json");

fn grease(value: u16) -> bool {
    value & 0x0f0f == 0x0a0a && value >> 8 == value & 0xff
}

/// The parts of one ClientHello the comparison reads.
#[derive(Debug)]
struct Hello {
    ciphers: Vec<u16>,
    cipher_grease: bool,
    extensions: Vec<(u16, Vec<u8>)>,
}

impl Hello {
    fn parse(record: &[u8]) -> Self {
        let mut input = Reader(record);
        assert_eq!(input.take(1), [22], "TLS handshake record");
        input.take(4);
        assert_eq!(input.take(1), [1], "ClientHello");
        input.take(3 + 2 + 32);
        let session = usize::from(input.take(1)[0]);
        input.take(session);
        let words = input.vector();
        let all: Vec<u16> = words
            .chunks(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .collect();
        let compression = usize::from(input.take(1)[0]);
        input.take(compression);
        let mut body = Reader(input.vector());
        let mut extensions = Vec::new();
        while !body.0.is_empty() {
            let kind = body.u16();
            extensions.push((kind, body.vector().to_vec()));
        }
        Hello {
            cipher_grease: all.iter().any(|v| grease(*v)),
            ciphers: all.into_iter().filter(|v| !grease(*v)).collect(),
            extensions,
        }
    }

    fn types(&self) -> BTreeSet<u16> {
        self.extensions
            .iter()
            .map(|(kind, _)| *kind)
            .filter(|kind| !grease(*kind))
            .collect()
    }

    fn body(&self, kind: u16) -> Option<&[u8]> {
        self.extensions
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, body)| body.as_slice())
    }

    fn alpn(&self) -> Vec<String> {
        let Some(body) = self.body(16) else {
            return Vec::new();
        };
        let mut list = Reader(&body[2..]);
        let mut protocols = Vec::new();
        while !list.0.is_empty() {
            let size = usize::from(list.take(1)[0]);
            protocols.push(String::from_utf8(list.take(size).to_vec()).unwrap());
        }
        protocols
    }

    /// A uint16 list extension (groups, signature algorithms) without GREASE.
    fn words(&self, kind: u16) -> Vec<u16> {
        let body = self.body(kind).expect("extension present");
        body[2..]
            .chunks(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .filter(|v| !grease(*v))
            .collect()
    }
}

impl Hello {
    /// supported_versions (a uint8-length list) without GREASE.
    fn versions(&self) -> Vec<u16> {
        let body = self.body(43).expect("supported_versions present");
        body[1..]
            .chunks(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .filter(|v| !grease(*v))
            .collect()
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> &'a [u8] {
        assert!(self.0.len() >= count, "truncated ClientHello");
        let (head, tail) = self.0.split_at(count);
        self.0 = tail;
        head
    }
    fn u16(&mut self) -> u16 {
        let bytes = self.take(2);
        u16::from_be_bytes([bytes[0], bytes[1]])
    }
    fn vector(&mut self) -> &'a [u8] {
        let count = usize::from(self.u16());
        self.take(count)
    }
}

/// Runs `connect` against a listener that keeps only the first TLS record.
async fn capture<F, Fut>(connect: F) -> Hello
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let listen = async move {
        let (mut socket, _) = server.accept().await.unwrap();
        let mut header = [0; 5];
        socket.read_exact(&mut header).await.unwrap();
        let mut record = header.to_vec();
        record.resize(
            5 + usize::from(u16::from_be_bytes([header[3], header[4]])),
            0,
        );
        socket.read_exact(&mut record[5..]).await.unwrap();
        record
    };
    let client = connect(address.to_string());
    let (record, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(listen, client)
    })
    .await
    .expect("localhost ClientHello capture timed out");
    Hello::parse(&record)
}

async fn https_hello() -> Hello {
    capture(|host| async move {
        let _ = fetch_html(&format!("https://{host}/"), FetchOptions::default()).await;
    })
    .await
}

async fn wss_hello() -> Hello {
    capture(|host| async move {
        let (_handle, mut events) = open(WsRequest {
            url: format!("wss://{host}/"),
            origin: format!("https://{host}"),
            ..WsRequest::default()
        });
        let _ = events.recv().await;
    })
    .await
}

fn control_types(control: &serde_json::Value, mode: &str) -> BTreeSet<u16> {
    control["views"][mode]["hello"]["extensions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["type"].as_u64().unwrap() as u16)
        .collect()
}

fn control_words(value: &serde_json::Value) -> Vec<u16> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect()
}

fn control_extension(control: &serde_json::Value, kind: u16) -> &serde_json::Value {
    control["views"]["wss"]["hello"]["extensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["type"] == kind)
        .unwrap()
}

#[tokio::test]
async fn wss_client_hello_is_the_https_hello_with_chromes_websocket_alpn() {
    let control: serde_json::Value = serde_json::from_str(CONTROL).unwrap();
    let https = https_hello().await;
    let wss = wss_hello().await;

    // (b) Chrome 156's wss: hello: same cipher suites, groups and signature algorithms
    // as its https hello, ALPN http/1.1 only.
    let chrome = &control["views"]["wss"];
    assert_eq!(wss.ciphers, control_words(&chrome["hello"]["ciphers"]));
    assert!(wss.cipher_grease, "cipher GREASE");
    assert_eq!(wss.alpn(), ["http/1.1"]);
    assert_eq!(
        serde_json::to_value(wss.alpn()).unwrap(),
        chrome["alpn"],
        "ALPN"
    );
    assert_eq!(
        wss.words(13),
        control_words(&chrome["signatureAlgorithms"]),
        "signature algorithms"
    );
    assert_eq!(
        wss.words(10),
        control_words(&control_extension(&control, 10)["values"]),
        "supported groups"
    );
    assert!(
        wss.extensions.iter().any(|(kind, _)| grease(*kind)),
        "extension GREASE"
    );

    // (a) surf's https hello, changed exactly where Chrome's https→wss change is.
    let removed: BTreeSet<u16> = control_types(&control, "https")
        .difference(&control_types(&control, "wss"))
        .copied()
        .collect();
    let added: BTreeSet<u16> = control_types(&control, "wss")
        .difference(&control_types(&control, "https"))
        .copied()
        .collect();
    assert_eq!(removed, BTreeSet::from([17613]), "Chrome drops only ALPS");
    assert!(added.is_empty());
    let expected: BTreeSet<u16> = https.types().difference(&removed).copied().collect();
    assert_eq!(wss.types(), expected, "extension set");
    assert_eq!(wss.ciphers, https.ciphers);
    assert_eq!(wss.words(13), https.words(13));
    assert_eq!(wss.words(10), https.words(10));
    assert_eq!(https.alpn(), ["h2", "http/1.1"]);
    assert_eq!(wss.versions(), https.versions(), "supported versions");
    // Every other extension is byte-identical; ALPN differs on purpose, and key shares,
    // ECH GREASE and the GREASE-bearing lists above are random per connection.
    for kind in wss.types() {
        if ![10, 13, 16, 43, 51, 65037].contains(&kind) {
            assert_eq!(wss.body(kind), https.body(kind), "extension {kind} body");
        }
    }
}
