//! Canvas `measureText` with a host text measurer installed (as mcp/napi install one) against
//! Chrome 156 (turbo-surf#133): `harness/detection/canvas-measure-text-probe.js`, recorded by
//! `harness/detection/canvas-measure-text-chrome-control.mjs` over the committed IBM Plex Mono
//! fixture loaded with `@font-face`. Every `TextMetrics` field comes from the face's glyph
//! outlines and font metrics (turbo-dom-browser-env's `set_canvas_fonts` registry), not from
//! size ratios, and the result is a branded `TextMetrics`. Process-global measurer, so this is
//! its own test binary.
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/canvas-measure-text-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/canvas-measure-text-chrome156.json");
const FONT: &str =
    "../../../harness/detection/fixtures/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf";

/// Chrome's glyph ink bounds come from Skia's outline bounds in `f32` (the control carries
/// their rounding noise); widths, font metrics and baselines match exactly.
const INK_TOLERANCE: f64 = 1e-4;

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    let digest = Sha256::digest(PROBE.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        control["fixtures"]["canvas-measure-text-probe.js"], hex,
        "control was captured from a different probe"
    );
    control
}

/// Serves the control's page and the TrueType fixture it declares.
async fn serve(page: String) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let font = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FONT)).unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (page, font) = (page.clone(), font.clone());
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
                let (kind, body) = if request.starts_with("GET /fonts/plex.ttf ") {
                    ("font/ttf", font)
                } else {
                    ("text/html", page.into_bytes())
                };
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    origin
}

fn same(key: &str, actual: &Value, expected: &Value) -> bool {
    match (actual.as_f64(), expected.as_f64()) {
        (Some(a), Some(e)) if key.starts_with("actual") => (a - e).abs() <= INK_TOLERANCE,
        (Some(a), Some(e)) => a == e,
        _ => actual == expected,
    }
}

fn compare(section: &str, keys: &[Value], actual: &Value, expected: &Value, out: &mut Vec<String>) {
    let (actual, expected) = (actual.as_array().unwrap(), expected.as_array().unwrap());
    assert_eq!(actual.len(), expected.len(), "{section} rows");
    for (row, chrome) in actual.iter().zip(expected) {
        let label = format!("{section} {} {}", chrome[0], chrome[1]);
        assert_eq!((&row[0], &row[1]), (&chrome[0], &chrome[1]), "{label}");
        for (index, key) in keys.iter().enumerate() {
            let key = key.as_str().unwrap();
            let (a, e) = (&row[2][index], &chrome[2][index]);
            if !same(key, a, e) {
                out.push(format!("{label} {key}: got {a}, Chrome {e}"));
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn measure_text_matches_chrome156_with_a_host_measurer() {
    // A host measurer that knows no web faces (the system-font measurer mcp/napi install):
    // canvas metrics must not come from it.
    turbo_surf_render::set_measure_fn(Box::new(|text, _, size| {
        (text.chars().count() as f64 * size * 0.5, size)
    }));
    let control = control();
    let page = control["fixtures"]["page"].as_str().unwrap().to_string();
    let origin = serve(page.clone()).await;
    let mut session = PageSession::open(&page, &format!("{origin}/"), "", "", 15000)
        .await
        .unwrap();
    let raw = session
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    session.close();
    let raw: Value = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(raw.as_str().unwrap()).unwrap();
    let expected = &control["result"];
    assert_eq!(actual["keys"], expected["keys"]);
    assert_eq!(actual["shape"], expected["shape"], "TextMetrics shape");
    let keys = expected["keys"].as_array().unwrap();
    let mut out = Vec::new();
    for section in ["metrics", "aligned"] {
        compare(
            section,
            keys,
            &actual[section],
            &expected[section],
            &mut out,
        );
    }
    assert!(
        out.is_empty(),
        "{} divergences:\n{}",
        out.len(),
        out.join("\n")
    );
}
