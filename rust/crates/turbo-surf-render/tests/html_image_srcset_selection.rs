//! turbo-dom#71 through surf's loader: an `<img>` loads the source turbo-dom-browser-env's
//! `imageHost.selectSource` picks (`<picture>` sources by media and type, `srcset` x and w
//! descriptors against `sizes`) and reports `imageHost.naturalSize` of the candidate it
//! decoded, as Chrome 156 does in the binding's control (1024x768; `load1` at
//! devicePixelRatio 1, `load2` at 2, `none1` with every image request failing). The local
//! server answers `/img/<W>x<H>/<name>.png` with a W x H PNG like the control's.
//! Its own binary: the viewport and devicePixelRatio come from the process-wide
//! fingerprint overrides.
#[path = "support/binding_control.rs"]
mod binding_control;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_dom_browser_env::test_support;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// A width x height 8-bit grayscale PNG (stored deflate blocks, no compression library).
fn png(width: u32, height: u32) -> Vec<u8> {
    let row = 1 + width as usize;
    let raw: Vec<u8> = (0..height as usize * row)
        .map(|index| if index % row == 0 { 0 } else { 0x80 })
        .collect();
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        let length = block.len() as u16;
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut header = width.to_be_bytes().to_vec();
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 0, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// `/img/<W>x<H>/<name>.png` -> (W, H).
fn image_size(path: &str) -> Option<(u32, u32)> {
    let size = path.strip_prefix("/img/")?.split('/').next()?;
    let (width, height) = size.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// Serve the control page at `/page/` (as the collector does) and its images (or a 404 for each when `load` is false).
async fn serve(html: String, load: bool) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let html = std::sync::Arc::new(html);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let html = html.clone();
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
                let (status, kind, body) = if path == "/page/" {
                    (200, "text/html", html.as_bytes().to_vec())
                } else {
                    match image_size(&path).filter(|_| load) {
                        Some((width, height)) => (200, "image/png", png(width, height)),
                        None => (404, "text/plain", Vec::new()),
                    }
                };
                let head = format!("HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    origin
}

#[test]
fn the_png_server_answers_the_control_sizes() {
    let bytes = png(300, 2);
    let probed = turbo_dom_layout::image::probe_natural(&bytes).unwrap();
    assert_eq!((probed.width, probed.height), (300, 2));
    assert_eq!(image_size("/img/100x50/a.png"), Some((100, 50)));
    assert_eq!(image_size("/img/x/a.png"), None);
    assert_eq!(crc32(b"IEND"), 0xae42_6082);
}

#[tokio::test(flavor = "current_thread")]
async fn image_sources_are_selected_and_sized_like_chrome156() {
    let cases: Value = serde_json::from_slice(test_support::HTML_IMAGE_SRCSET_CASES_JSON).unwrap();
    assert_eq!(cases["viewport"], serde_json::json!([1024, 768]));
    let control: Value =
        serde_json::from_str(test_support::HTML_IMAGE_SRCSET_CHROME156_JSON).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        binding_control::sha(test_support::HTML_IMAGE_SRCSET_PROBE_JS.as_bytes())
    );
    assert_eq!(
        control["casesSha256"],
        binding_control::sha(test_support::HTML_IMAGE_SRCSET_CASES_JSON)
    );
    let probe = test_support::HTML_IMAGE_SRCSET_PROBE_JS
        .trim_end()
        .trim_end_matches(';');
    let mut mismatches = Vec::new();
    for (label, ratio, load) in [("load1", 1, true), ("load2", 2, true), ("none1", 1, false)] {
        turbo_surf_render::set_fingerprint(
            &serde_json::json!({"innerWidth": 1024, "innerHeight": 768, "outerWidth": 1024,
                "outerHeight": 768, "devicePixelRatio": ratio})
            .to_string(),
        );
        let html = cases["html"].as_str().unwrap().to_string();
        let origin = serve(html.clone(), load).await;
        let mut page =
            turbo_surf_render::PageSession::open(&html, &format!("{origin}/page/"), "", "", 60_000)
                .await
                .unwrap();
        let actual: Value =
            serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
        let expected = &control["result"][label];
        for (name, value) in expected.as_object().unwrap() {
            if actual.get(name) != Some(value) {
                mismatches.push(format!(
                    "{label}.{name}: expected {value}, got {}",
                    actual[name]
                ));
            }
        }
    }
    turbo_surf_render::set_fingerprint("{}");
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}

/// turbo-dom#71, surf's mutation consumer: a `<picture>`'s image reselects (and loads the
/// new choice) when a `<source>` attribute changes, a `<source>` is inserted and one is
/// removed, against `fixtures/html_picture_mutations_chrome156.json`
/// (`fixture-gen/html-picture-mutations-chrome.mjs`).
#[tokio::test(flavor = "current_thread")]
async fn picture_sources_reselect_on_mutations_like_chrome156() {
    const PROBE: &str = include_str!("fixtures/html_picture_mutations_probe.js");
    const CASES: &str = include_str!("fixtures/html_picture_mutations_cases.json");
    let control: Value = serde_json::from_str(include_str!(
        "fixtures/html_picture_mutations_chrome156.json"
    ))
    .unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        binding_control::sha(PROBE.as_bytes())
    );
    assert_eq!(
        control["casesSha256"],
        binding_control::sha(CASES.as_bytes())
    );
    let cases: Value = serde_json::from_str(CASES).unwrap();
    // Every candidate here is 1x, so the choice does not depend on the viewport or the
    // devicePixelRatio the other test of this binary sets.
    let html = cases[0]["html"].as_str().unwrap().to_string();
    let origin = serve(html.clone(), true).await;
    let mut page =
        turbo_surf_render::PageSession::open(&html, &format!("{origin}/page/"), "", "", 60_000)
            .await
            .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(PROBE.trim_end().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, control["result"][0]["result"]);
}
