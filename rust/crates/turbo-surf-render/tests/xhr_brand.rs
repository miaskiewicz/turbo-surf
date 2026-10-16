use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn genuine_xhr_brand_descriptors_and_private_receivers_match_chrome() {
    let source = include_str!("../../../../harness/detection/xhr-brand-probe.js");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/xhr-brand-chrome155.json"
    ))
    .unwrap();
    assert_eq!(expected["browser"], "Chrome/155.0.8059.80");
    assert_eq!(expected["v8"], "15.5.35.23");
    assert_eq!(expected["errors"], serde_json::json!([]));
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    for (key, source) in [
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/xhr-brand-chrome.mjs").as_slice(),
        ),
        (
            "serverSha256",
            include_bytes!("../../../../harness/detection/xhr-brand-server.mjs").as_slice(),
        ),
    ] {
        assert_eq!(expected[key], format!("{:x}", Sha256::digest(source)));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|part| part == b"\r\n\r\n") {
            let mut part = [0; 1024];
            let count = socket.read(&mut part).await.unwrap();
            assert!(count > 0 && request.len() + count <= 16384);
            request.extend_from_slice(&part[..count]);
        }
        assert!(request.starts_with(b"GET /xhr HTTP/1.1\r\n"));
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 12\r\nConnection: close\r\n\r\nxhr-response").await.unwrap();
    });
    let mut page = PageSession::open("<!doctype html><body>XHR control", &origin, "", "", 15000)
        .await
        .unwrap();
    let probe = source.trim().trim_end_matches(';');
    let actual = page
        .eval_expression(&format!("(async()=>JSON.stringify(await ({probe})))()"))
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
    page.close();
    server.await.unwrap();
}
