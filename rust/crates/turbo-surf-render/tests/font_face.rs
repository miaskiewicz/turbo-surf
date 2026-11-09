//! `@font-face` and the CSS Font Loading API (turbo-surf#130) against the Chrome 156
//! control: faces fetched through the page's network stack (CORS, CSP `font-src`),
//! WOFF/WOFF2 decoded, registered for live layout and canvas `measureText`, and
//! `document.fonts` timing. Fonts come from committed fixtures, never system fonts.
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/font-face-probe.js");
const READY: &str = include_str!("../../../../harness/detection/font-face-ready-probe.js");
const ACCESSOR: &str = include_str!("../../../../harness/detection/font-face-accessor-probe.js");
const RANGE: &str =
    include_str!("../../../../harness/detection/fixtures/embedded-font-range-probe.js");
const CASES: &str = include_str!("../../../../harness/detection/fixtures/font-face-cases.json");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/font-face-chrome156.json");
const FONTS: &str = "../../../harness/detection/fixtures/fonts/ibm-plex-mono";

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .ends_with("Chrome/156.0.8078.47"));
    for (name, text) in [
        ("font-face-probe.js", PROBE),
        ("font-face-ready-probe.js", READY),
        ("font-face-accessor-probe.js", ACCESSOR),
        ("fixtures/embedded-font-range-probe.js", RANGE),
        ("fixtures/font-face-cases.json", CASES),
    ] {
        assert_eq!(
            control["fixtures"][name],
            format!("{:x}", Sha256::digest(text)),
            "stale control: {name}"
        );
    }
    control["result"].clone()
}

fn font(path: &str) -> Option<(Vec<u8>, &'static str)> {
    let (file, kind) = match path {
        "/fonts/plex.ttf" => ("IBMPlexMono-Regular.ttf", "font/ttf"),
        "/fonts/plex.woff" => ("IBMPlexMono-Regular.woff", "font/woff"),
        "/fonts/plex.woff2" => ("IBMPlexMono-Regular.woff2", "font/woff2"),
        _ => return None,
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FONTS);
    Some((std::fs::read(dir.join(file)).unwrap(), kind))
}

/// A font server; `cross` adds `Access-Control-Allow-Origin: *` for `?cors=1` only.
async fn serve(cross: bool) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut buffer = [0; 1024];
                while !bytes.windows(4).any(|row| row == b"\r\n\r\n") {
                    let count = socket.read(&mut buffer).await.unwrap_or(0);
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let request = String::from_utf8_lossy(&bytes);
                let target = request.split_whitespace().nth(1).unwrap_or("/");
                let path = target.split('?').next().unwrap();
                let cors = if cross && target.contains("cors=1") {
                    "Access-Control-Allow-Origin: *\r\n"
                } else {
                    ""
                };
                let (status, kind, body) = match font(path) {
                    Some((body, kind)) => ("200 OK", kind, body),
                    None => ("404 Not Found", "text/plain", b"missing".to_vec()),
                };
                let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    origin
}

fn divergences(path: String, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Object(left), Value::Object(right)) => {
            let mut keys: Vec<_> = left.keys().chain(right.keys()).collect();
            keys.sort();
            keys.dedup();
            let missing = Value::String("<absent>".into());
            for key in keys {
                divergences(
                    format!("{path}/{key}"),
                    left.get(key).unwrap_or(&missing),
                    right.get(key).unwrap_or(&missing),
                    out,
                );
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                divergences(format!("{path}/{index}"), left, right, out);
            }
        }
        _ if actual != expected => out.push(format!("{path} = {actual} (chrome {expected})")),
        _ => {}
    }
}

async fn eval(page: &mut PageSession, probe: &str) -> Value {
    let raw = page
        .eval_json_expression(probe.trim().trim_end_matches(';'))
        .await
        .unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    serde_json::from_str(value.as_str().unwrap()).unwrap()
}

async fn run_case(name: &str) -> Value {
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let case = cases["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|page| page["name"] == name)
        .unwrap()
        .clone();
    let source = serve(false).await;
    let cross = serve(true).await;
    let html = case["html"].as_str().unwrap().replace("{cross}", &cross);
    let headers = case["headers"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    (
                        row[0].as_str().unwrap().to_string(),
                        row[1].as_str().unwrap().to_string(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut page = PageSession::open_with_response_headers(
        &html,
        &format!("{source}/{name}"),
        "",
        "",
        20000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let mut result = Value::Null;
    for probe in case["probes"].as_array().unwrap() {
        result = eval(
            &mut page,
            match probe.as_str().unwrap() {
                "font-face-probe.js" => PROBE,
                "font-face-ready-probe.js" => READY,
                "font-face-accessor-probe.js" => ACCESSOR,
                _ => RANGE,
            },
        )
        .await;
    }
    page.close();
    result
}

/// Known gap (pinned exactly, so a fix or a new divergence fails here): Chrome 156 gives
/// an `@font-face` face of IBM Plex Mono a 25px inline content height at 20px where
/// turbo-dom's HTML metrics give 26px, as for the installed face (Chrome 155's
/// installed-font control also reads 26). The web-font metric policy belongs in
/// turbo-dom-layout's `FontFace::from_web_font_bytes`.
const WEB_FONT_HEIGHT: &[&str] = &[
    "/ready/heights/TTF = 26 (chrome 25)",
    "/ready/heights/WOFF2 = 26 (chrome 25)",
];
// turbo-dom#377 (release/0.8.0 d1237332) aligned the live-layout content heights for
// the embedded web face: the heading / contentsRange rects now match Chrome at 25px.
// The inline content box's y-position still trails by 0.5 px (11.5 / 7.5 / 43.5 / 79.5
// vs Chrome's 11 / 7 / 43 / 79): a separate, still-open live-layout gap owned by
// turbo-dom. `rows/link/rects/*/3 = 26` in RANGE_MIXED is the plain-font row (the
// mixed-range case paints one row in Plex, another in the default serif), which the
// web-font metric policy does not reach.
const RANGE_BLOCK_HEIGHT: &[&str] = &[
    "/rows/heading/contentsRange/bounds/1 = 11.5 (chrome 11)",
    "/rows/heading/contentsRange/rects/0/1 = 11.5 (chrome 11)",
    "/rows/link/contentsRange/bounds/1 = 11.5 (chrome 11)",
    "/rows/link/contentsRange/rects/1/1 = 11.5 (chrome 11)",
];
const RANGE_MIXED_HEIGHT: &[&str] = &[
    "/rows/heading/contentsRange/bounds/1 = 43.5 (chrome 43)",
    "/rows/heading/contentsRange/rects/0/1 = 43.5 (chrome 43)",
    "/rows/link/bounds/3 = 98 (chrome 97)",
    "/rows/link/contentsRange/bounds/1 = 7.5 (chrome 7)",
    "/rows/link/contentsRange/rects/0/1 = 7.5 (chrome 7)",
    "/rows/link/contentsRange/rects/2/1 = 43.5 (chrome 43)",
    "/rows/link/contentsRange/rects/3/1 = 79.5 (chrome 79)",
    "/rows/link/rects/0/3 = 26 (chrome 25)",
    "/rows/link/rects/2/3 = 26 (chrome 25)",
];

async fn assert_case(name: &str, known: &[&str]) {
    let expected = control()[name].clone();
    let actual = run_case(name).await;
    let mut found = Vec::new();
    divergences(String::new(), &actual, &expected, &mut found);
    assert_eq!(found, known, "{name} diverges from Chrome");
}

#[tokio::test(flavor = "current_thread")]
async fn font_face_loading_matches_chrome156() {
    assert_case("plain", WEB_FONT_HEIGHT).await;
}

#[tokio::test(flavor = "current_thread")]
async fn font_face_csp_font_src_matches_chrome156() {
    assert_case("csp", WEB_FONT_HEIGHT).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "FIXME(#287, turbo-dom#478): iframe inline onload compiles against parent realm; fix in turbo-dom 0.8.2 + surf 0.8.2"]
async fn embedded_web_font_block_range_matches_chrome156() {
    assert_case("range-block-only", RANGE_BLOCK_HEIGHT).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "FIXME(#287, turbo-dom#478): iframe inline onload compiles against parent realm; fix in turbo-dom 0.8.2 + surf 0.8.2"]
async fn embedded_web_font_mixed_range_matches_chrome156() {
    assert_case("range-mixed", RANGE_MIXED_HEIGHT).await;
}

/// Chrome 156: the style update during loading starts a used `@font-face` face, in the
/// top document and in a frame, and the face's load delays the window load event; an
/// unused face stays unloaded (turbo-surf#166). No script reads layout here.
#[tokio::test(flavor = "current_thread")]
async fn used_css_faces_load_before_the_load_event_without_a_layout_read() {
    let source = serve(false).await;
    let face = r#"@font-face{font-family:Plex;src:url(/fonts/plex.woff2) format("woff2")}"#;
    let cases = [
        (
            format!(
                r#"<!doctype html><style>{face}</style><p style="font-family:Plex">font</p><script>addEventListener("load",()=>{{window.__fontLoad="load:"+[...document.fonts].map(f=>f.status)}})</script>"#
            ),
            "load:loaded",
        ),
        (
            format!(
                r#"<!doctype html><iframe id="f" srcdoc='<style>{face}</style><p style="font-family:Plex">font</p>'></iframe><script>addEventListener("load",()=>{{window.__fontLoad="load:"+[...document.getElementById("f").contentDocument.fonts].map(f=>f.status)}})</script>"#
            ),
            "load:loaded",
        ),
        (
            format!(
                r#"<!doctype html><style>{face}</style><p>font</p><script>addEventListener("load",()=>{{window.__fontLoad="load:"+[...document.fonts].map(f=>f.status)}})</script>"#
            ),
            "load:unloaded",
        ),
    ];
    for (html, expected) in cases {
        let mut page = PageSession::open(&html, &format!("{source}/page"), "", "", 20000)
            .await
            .unwrap();
        let actual = page
            .eval_json_expression("window.__fontLoad")
            .await
            .unwrap();
        page.close();
        assert_eq!(actual, format!("\"{expected}\""), "{html}");
    }
}

/// `document.fonts` is a Web-IDL accessor in Chrome: `get fonts`, length 0, native
/// source, not constructible, and an `Illegal invocation` brand check. A JS closure
/// here (name "get", readable source) is a bot-detection tell.
#[tokio::test(flavor = "current_thread")]
async fn document_fonts_accessor_matches_chrome156() {
    assert_case("accessor", &[]).await;
}
