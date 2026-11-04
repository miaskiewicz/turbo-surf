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
const ROUTES: [(&str, &str, &str); 14] = [
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
    (
        "/latin1-decl",
        "application/xml",
        r#"<?xml version="1.0" encoding="ISO-8859-1"?><a>café</a>"#,
    ),
    ("/utf16-bom", "application/xml", "<a>é€</a>"),
    (
        "/header-latin1",
        "application/xml; charset=ISO-8859-1",
        "<a>café</a>",
    ),
    (
        "/bom-beats-header",
        "application/xml; charset=ISO-8859-1",
        "<a>é</a>",
    ),
    (
        "/header-vs-decl",
        "application/xml; charset=UTF-8",
        r#"<?xml version="1.0" encoding="ISO-8859-1"?><a>café</a>"#,
    ),
    (
        "/utf16-decl",
        "application/xml",
        r#"<?xml version="1.0" encoding="UTF-16"?><a>café</a>"#,
    ),
    (
        "/literal-parsererror",
        "application/xml",
        r#"<a><parsererror xmlns="http://www.w3.org/1999/xhtml">x</parsererror></a>"#,
    ),
];

/// A route's body bytes, encoded as xhr-document-server.mjs's `routeBytes` does.
fn route_bytes(path: &str, body: &str) -> Vec<u8> {
    match path {
        "/latin1-decl" | "/header-latin1" | "/header-vs-decl" => {
            body.chars().map(|c| c as u8).collect()
        }
        "/utf16-bom" => std::iter::once('\u{feff}')
            .chain(body.chars())
            .collect::<String>()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
        "/bom-beats-header" => [&[0xef, 0xbb, 0xbf][..], body.as_bytes()].concat(),
        _ => body.as_bytes().to_vec(),
    }
}

fn js_string(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn response_xml_and_document_responses_match_chrome156() {
    let expected: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    assert!(expected["browser"]
        .as_str()
        .unwrap()
        .ends_with("Chrome/156.0.8078.47"));
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
                (
                    "text/html",
                    b"<!doctype html><body>XHR document control".to_vec(),
                ),
                |r| (r.1, route_bytes(r.0, r.2)),
            );
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
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

/// A document response is bounded by the 8 MiB body cap: one byte over it is a
/// network error, so nothing is parsed (`responseXML` and the "document" response
/// are null, the text is empty); one at the cap still parses.
#[tokio::test(flavor = "current_thread")]
async fn document_responses_are_bounded_by_the_body_cap() {
    const CAP: usize = 8 * 1024 * 1024;
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
            let line = String::from_utf8_lossy(&request).into_owned();
            let size = if line.contains("GET /over") {
                Some(CAP + 1)
            } else if line.contains("GET /at") {
                Some(CAP)
            } else {
                None
            };
            let (mime, body) = match size {
                Some(size) => {
                    let mut body = b"<a>".to_vec();
                    body.resize(size - 4, b'x');
                    body.extend_from_slice(b"</a>");
                    ("application/xml", body)
                }
                None => ("text/html", b"<!doctype html><body>cap".to_vec()),
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&body).await;
        }
    });
    let mut page = PageSession::open("<!doctype html><body>cap", &origin, "", "", 60000)
        .await
        .unwrap();
    let actual = page
        .eval_expression(
            r#"(async()=>{
              const get=(path,type)=>new Promise(done=>{const x=new XMLHttpRequest();x.open("GET",path);if(type)x.responseType=type;x.onloadend=()=>done(x);x.send();});
              const at=await get("/at","");
              const over=await get("/over","");
              const doc=await get("/over","document");
              return JSON.stringify([at.responseXML!==null,at.responseText.length,over.responseXML,over.responseText.length,doc.response]);
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(actual, format!("[true,{CAP},null,0,null]"));
    page.close();
    server.abort();
}
