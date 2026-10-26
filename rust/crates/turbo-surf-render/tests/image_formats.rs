//! turbo-surf#30: live `<img>` loads decide success and `naturalWidth`/`naturalHeight`
//! with turbo-dom-layout's shared image layer (`image::decode_data_url` for every `data:`
//! form, `image::probe_natural` / the raster probe for the bytes), checked per format
//! against a Chrome 156 control: PNG/JPEG/GIF/WebP/BMP/ICO/CUR/AVIF sizes, SVG natural
//! sizes (served as `image/svg+xml` only), percent-encoded and raw `data:` URLs, wrong
//! MIME types, truncated bodies and HTTP error statuses. The cases are built by
//! `fixture-gen/make-image-format-cases.py`; the control by
//! `fixture-gen/html-image-formats-chrome.mjs`.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("fixtures/image_formats_probe.js");
const CASES: &str = include_str!("fixtures/image_formats_cases.json");
const CONTROL: &str = include_str!("fixtures/image_formats_chrome156.json");

/// Known gaps of the shared probe (turbo-dom-layout `probe_natural`), pinned to surf's
/// value per section and id. Blink reports a size only once its decoder has read past the
/// header — a JPEG's `SOS`, a GIF's first image descriptor, a WebP's complete frame
/// header (the PNG rule, the first `IDAT`, already matches) — while the probe sizes these
/// truncated bodies from the header alone; and Blink rejects an `image/svg+xml` document
/// whose root `<svg>` is not in the SVG namespace, which usvg parses.
const GAPS: &[(&str, &str, &str)] = &[
    ("parsed", "jpeg-sof-only", "[true,5,4]"),
    ("parsed", "jpeg-before-sos", "[true,5,4]"),
    ("parsed", "gif-screen-only", "[true,6,5]"),
    ("parsed", "webp-header", "[true,7,6]"),
    ("parsed", "svg-no-namespace", "[true,40,30]"),
    ("created", "jpeg-sof-only", r#"["load",true,5,4]"#),
    ("created", "jpeg-before-sos", r#"["load",true,5,4]"#),
    ("created", "gif-screen-only", r#"["load",true,6,5]"#),
    ("created", "webp-header", r#"["load",true,7,6]"#),
    ("created", "svg-no-namespace", r#"["load",true,40,30]"#),
];

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Serve each http case at `/<id>` with its status and Content-Type.
async fn serve(cases: &[Value]) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let routes: Vec<(String, u16, String, Vec<u8>)> = cases
        .iter()
        .map(|case| {
            let body = base64::engine::general_purpose::STANDARD
                .decode(case["body"].as_str().unwrap())
                .unwrap();
            (
                format!("/{}", case["id"].as_str().unwrap()),
                case["status"].as_u64().unwrap() as u16,
                case["contentType"].as_str().unwrap().to_string(),
                body,
            )
        })
        .collect();
    let routes = std::sync::Arc::new(routes);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut chunk = [0; 1024];
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&chunk[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, kind, body) = routes.iter().find(|(route, ..)| *route == path).map_or(
                    (404, String::new(), Vec::new()),
                    |(_, status, kind, body)| (*status, kind.clone(), body.clone()),
                );
                let kind = if kind.is_empty() {
                    String::new()
                } else {
                    format!("Content-Type: {kind}\r\n")
                };
                let head = format!(
                    "HTTP/1.1 {status} X\r\n{kind}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    origin
}

#[tokio::test(flavor = "current_thread")]
async fn live_image_loads_decode_and_size_each_format_like_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        sha(PROBE.as_bytes()),
        "stale probe control"
    );
    assert_eq!(
        control["casesSha256"],
        sha(CASES.as_bytes()),
        "stale cases control"
    );
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let origin = serve(cases["http"].as_array().unwrap()).await;
    let mut page = PageSession::open(
        cases["html"].as_str().unwrap(),
        &format!("{origin}/"),
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(PROBE.trim_end().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    let expected = &control["result"];
    let mut mismatches = Vec::new();
    for section in ["parsed", "created", "http"] {
        let rows = expected[section].as_object().unwrap();
        assert_eq!(
            actual[section].as_object().unwrap().len(),
            rows.len(),
            "{section}"
        );
        for (id, chrome) in rows {
            let got = &actual[section][id];
            match GAPS.iter().find(|(s, gap, _)| *s == section && gap == id) {
                Some((_, _, pinned)) => {
                    let pinned: Value = serde_json::from_str(pinned).unwrap();
                    assert_eq!(*got, pinned, "{section}/{id}: known gap changed");
                    assert_ne!(*chrome, pinned, "{section}/{id}: known gap is fixed");
                }
                None if got != chrome => {
                    mismatches.push(format!("{section}/{id}: expected {chrome}, got {got}"))
                }
                None => {}
            }
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}
