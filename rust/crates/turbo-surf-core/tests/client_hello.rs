//! Offline wire regression against the Chrome 154-recipe localhost ClientHello.
//! The listener closes before certificate exchange or HTTP; no remote traffic.
#![cfg(feature = "impersonate")]

use tokio::io::AsyncReadExt;
use turbo_surf_core::net::{fetch_html, transport_chrome_major, FetchOptions};

fn take<'a>(input: &mut &'a [u8], count: usize) -> &'a [u8] {
    assert!(input.len() >= count, "truncated ClientHello");
    let (head, tail) = input.split_at(count);
    *input = tail;
    head
}

fn u16_at(input: &mut &[u8]) -> u16 {
    let bytes = take(input, 2);
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn vector<'a>(input: &mut &'a [u8]) -> &'a [u8] {
    let count = usize::from(u16_at(input));
    take(input, count)
}

fn grease(value: u16) -> bool {
    value & 0x0f0f == 0x0a0a && value >> 8 == value & 0xff
}

#[tokio::test]
async fn chrome154_client_hello_has_real_anchors_and_signature_grease() {
    assert_eq!(
        transport_chrome_major(),
        Some(turbo_surf_core::browser::DEFAULT_MAJOR)
    );
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let capture = tokio::spawn(async move {
        let (mut socket, _) = server.accept().await.unwrap();
        let mut header = [0; 5];
        socket.read_exact(&mut header).await.unwrap();
        assert_eq!(header[0], 22, "expected TLS handshake record");
        let mut record = vec![0; usize::from(u16::from_be_bytes([header[3], header[4]]))];
        socket.read_exact(&mut record).await.unwrap();
        record
    });
    let url = format!("https://{address}/");
    let request = fetch_html(&url, FetchOptions::default());
    let (_, record) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(request, capture)
    })
    .await
    .expect("localhost TLS capture timed out");
    let record = record.unwrap();
    assert_eq!(record[0], 1, "expected ClientHello handshake");
    let mut input = &record[4..];
    take(&mut input, 34); // legacy version and random
    let session_len = usize::from(take(&mut input, 1)[0]);
    take(&mut input, session_len);
    let ciphers: Vec<u16> = vector(&mut input)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .filter(|v| !grease(*v))
        .collect();
    assert_eq!(
        ciphers,
        [
            4865, 4866, 4867, 49195, 49199, 49196, 49200, 52393, 52392, 49171, 49172, 156, 157, 47,
            53
        ]
    );
    let compression_len = usize::from(take(&mut input, 1)[0]);
    take(&mut input, compression_len);
    let mut extensions = vector(&mut input);
    let mut anchors = None;
    let mut sigalgs = None;
    while !extensions.is_empty() {
        let kind = u16_at(&mut extensions);
        let payload = vector(&mut extensions);
        match kind {
            51764 => anchors = Some(payload),
            13 => sigalgs = Some(payload),
            _ => {}
        }
    }
    let anchors = anchors.expect("Chrome 154 trust anchors missing");
    let expected = include_str!("fixtures/chrome154-trust-anchors.hex").trim();
    let observed: String = anchors.iter().map(|v| format!("{v:02x}")).collect();
    assert_eq!(
        observed, expected,
        "public Chromium root IDs differ from local Chrome 154"
    );
    let mut sigalgs = sigalgs.expect("signature algorithms missing");
    let algorithms: Vec<u16> = vector(&mut sigalgs)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect();
    assert!(grease(algorithms[0]), "missing signature-algorithm GREASE");
    assert_eq!(
        &algorithms[1..],
        [2308, 2309, 2310, 1027, 2052, 1025, 1283, 2053, 1281, 2054, 1537]
    );
}
