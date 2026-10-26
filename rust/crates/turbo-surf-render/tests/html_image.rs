//! turbo-surf#32 / turbo-dom#23: HTMLImageElement load state through surf's real image
//! loader. The binding owns the element (the prototype accessors complete/naturalWidth/
//! naturalHeight/currentSrc, the reflected src, decode() and the native Image() factory)
//! and its Chrome 156 control; surf starts loads from the document's mutation consumer
//! and publishes each one through the private `imageHost` record, so no image ever
//! carries own properties.
#[path = "support/binding_control.rs"]
mod binding_control;
use binding_control::{matches_control, matches_control_with_gaps, sha};
use serde_json::Value;
use turbo_dom_browser_env::test_support;

#[tokio::test(flavor = "current_thread")]
async fn image_state_through_the_surf_loader_matches_chrome156() {
    matches_control(
        test_support::HTML_IMAGE_PROBE_JS,
        test_support::HTML_IMAGE_CASES_JSON,
        test_support::HTML_IMAGE_CHROME156_JSON,
        |case| case["documentBase"].as_str().unwrap(),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn src_and_decode_are_prototype_members_that_load_like_chrome156() {
    // Known gap (turbo-surf#42): the binding's decode() rejects with the DOMException it
    // captured at install, and surf installs its own DOMException afterwards, so the
    // EncodingError is not `instanceof DOMException` (name/message match).
    let encoding = serde_json::json!([
        "throw",
        "EncodingError",
        "The source image cannot be decoded.",
        false
    ]);
    let broken = serde_json::json!({"event": "error", "decoded": encoding, "complete": true,
        "naturalWidth": 0, "naturalHeight": 0, "currentSrc": true, "own": []});
    matches_control_with_gaps(
        include_str!("fixtures/html_image_members_probe.js"),
        include_bytes!("fixtures/html_image_members_cases.json"),
        include_str!("fixtures/html_image_members_chrome156.json"),
        |case| case["url"].as_str().unwrap(),
        &[("decodeEmpty", encoding.clone()), ("createdBroken", broken)],
    )
    .await;
}

/// turbo-dom#23 (surf side): loads start from the document's mutation consumer, so an
/// image loads whenever its src changes or a tree inserts it, connected or not
/// (`new Image().src`, detached innerHTML, contextual fragments), never in an inert
/// document, and a src change or removal restarts or clears its state like Chrome.
#[tokio::test(flavor = "current_thread")]
async fn images_load_from_mutations_connected_or_not_like_chrome156() {
    matches_control(
        include_str!("fixtures/html_image_loads_probe.js"),
        include_bytes!("fixtures/html_image_loads_cases.json"),
        include_str!("fixtures/html_image_loads_chrome156.json"),
        |case| case["url"].as_str().unwrap(),
    )
    .await;
}

/// turbo-surf#40: an image loads the srcset candidate HTML's source selection picks for
/// surf's devicePixelRatio (2) and 1280px viewport — density and `w` descriptors (`sizes`,
/// default 100vw), `src` as the 1x candidate — and reports its density-corrected natural
/// size, as Chrome 156 does over the same uncached candidates.
#[tokio::test(flavor = "current_thread")]
async fn srcset_candidates_are_selected_like_chrome156() {
    use base64::Engine;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let cases: Value =
        serde_json::from_slice(include_bytes!("fixtures/html_image_srcset_cases.json")).unwrap();
    let images: std::collections::HashMap<String, Vec<u8>> = cases[0]["images"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, data)| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.as_str().unwrap())
                .unwrap();
            (name.clone(), bytes)
        })
        .collect();
    let images = std::sync::Arc::new(images);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let images = images.clone();
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
                let path = text.split_whitespace().nth(1).unwrap_or("/");
                let image = path
                    .strip_suffix(".png")
                    .and_then(|stem| stem.rsplit('/').next())
                    .and_then(|name| images.get(name));
                let (status, body) = image.map_or((404, Vec::new()), |bytes| (200, bytes.clone()));
                let head = format!("HTTP/1.1 {status} X\r\nContent-Type: image/png\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    let mut case = cases[0].clone();
    case["url"] = Value::String(format!("{origin}/"));
    let cases = serde_json::to_vec(&serde_json::json!([case])).unwrap();
    let control: Value =
        serde_json::from_str(include_str!("fixtures/html_image_srcset_chrome156.json")).unwrap();
    assert_eq!(
        control["casesSha256"],
        sha(include_bytes!("fixtures/html_image_srcset_cases.json"))
    );
    let mut control = control;
    control["casesSha256"] = Value::String(sha(&cases));
    matches_control(
        include_str!("fixtures/html_image_srcset_probe.js"),
        &cases,
        &control.to_string(),
        |case| case["url"].as_str().unwrap(),
    )
    .await;
}

/// turbo-surf#38: a data: image loads synchronously — complete with its natural size as
/// soon as its source is set (parser, script-created, fragment-parsed and connected
/// images alike); a broken script-set one stays pending until its error event, while a
/// broken parser one is already complete at the first script read.
#[tokio::test(flavor = "current_thread")]
async fn data_images_decode_synchronously_like_chrome156() {
    matches_control(
        include_str!("fixtures/html_image_sync_probe.js"),
        include_bytes!("fixtures/html_image_sync_cases.json"),
        include_str!("fixtures/html_image_sync_chrome156.json"),
        |case| case["url"].as_str().unwrap(),
    )
    .await;
}

/// turbo-surf#32 item 2 / #38: render()'s script runs over the parsed document, so parser
/// images have started loading by the time it reads them, and a data: image is decoded
/// synchronously (complete with its natural size on the first read; its load event still
/// fires later), as for an inline script at the end of the body in Chrome.
#[tokio::test(flavor = "current_thread")]
async fn parser_images_load_before_a_render_script_like_chrome156() {
    let probe = include_str!("fixtures/html_image_render_probe.js");
    let page = include_str!("fixtures/html_image_render_page.html");
    let control: Value =
        serde_json::from_str(include_str!("fixtures/html_image_render_chrome156.json")).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(control["probeSha256"], sha(probe.as_bytes()));
    assert_eq!(control["pageSha256"], sha(page.as_bytes()));
    let html = turbo_surf_render::render_page(page, "https://html-image.test/", probe)
        .await
        .unwrap();
    let title = html
        .split_once("<title>")
        .and_then(|(_, rest)| rest.split_once("</title>"))
        .map(|(title, _)| title.replace("&quot;", "\""))
        .unwrap();
    let actual: Value = serde_json::from_str(&title).unwrap();
    assert_eq!(actual, control["result"]);
}
