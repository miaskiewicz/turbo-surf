//! XHR `responseXML` and `responseType = "document"` (turbo-dom#186) against the Chrome
//! 156 control captured by harness/detection/xhr-document-chrome.mjs.
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/xhr-document-probe.js");
const SERVER: &str = include_str!("../../../../harness/detection/xhr-document-server.mjs");
const FIXTURE: &str =
    include_str!("../../../../harness/detection/fixtures/xhr-document-chrome156.json");

/// The server's routes: path -> (content type, body), as in xhr-document-server.mjs.
const ROUTES: [(&str, &str, &str); 7] = [
    (
        "/xml",
        "application/xml",
        r#"<?xml version="1.0"?><a xmlns:p="urn:p"><b p:c="1">t</b></a>"#,
    ),
    ("/text-xml", "text/xml", "<a><![CDATA[x<y]]></a>"),
    (
        "/svg",
        "image/svg+xml",
        r#"<svg xmlns="http://www.w3.org/2000/svg"><g/></svg>"#,
    ),
    (
        "/xhtml",
        "application/xhtml+xml",
        r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>x</body></html>"#,
    ),
    ("/bad", "application/xml", "<a><b></a>"),
    ("/html", "text/html", "<!doctype html><title>t</title><p>x"),
    ("/plain", "text/plain", "<a>plain</a>"),
];

fn js_string(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn response_xml_and_document_responses_match_chrome156() {
    let expected: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(expected["browser"], "Chrome/156.0.8078.47");
    assert_eq!(expected["errors"], serde_json::json!([]));
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    assert_eq!(
        expected["serverSha256"],
        format!("{:x}", Sha256::digest(SERVER))
    );
    for (path, mime, body) in ROUTES {
        let route = SERVER
            .lines()
            .skip_while(|line| !line.contains(&js_string(path)))
            .take(4)
            .collect::<String>();
        assert!(
            route.contains(&js_string(mime)) && route.contains(body),
            "server route {path}"
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut part = [0; 1024];
                let count = socket.read(&mut part).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&part[..count]);
            }
            let line = String::from_utf8_lossy(&request);
            let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (mime, body) = ROUTES.iter().find(|route| route.0 == path).map_or(
                ("text/html", "<!doctype html><body>XHR document control"),
                |r| (r.1, r.2),
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let mut page = PageSession::open(
        "<!doctype html><body>XHR document control",
        &origin,
        "",
        "",
        15000,
    )
    .await
    .unwrap();
    let probe = PROBE.trim().trim_end_matches(';');
    let actual = page
        .eval_expression(&format!("(async()=>JSON.stringify(await ({probe})))()"))
        .await
        .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    for (key, value) in expected["result"].as_object().unwrap() {
        assert_eq!(&actual[key], value, "{key}");
    }
    assert_eq!(actual, expected["result"]);
    page.close();
    server.abort();
}
