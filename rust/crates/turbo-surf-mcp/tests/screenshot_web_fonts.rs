//! turbo-surf#142: the static screenshot loads the page's `@font-face` faces (a linked
//! sheet's, resolved against the sheet's URL past a failing first source, and an inline
//! `<style>`'s) over the session and lays the text out and paints it with them, as Chrome 156
//! does: IBM Plex Mono's advances and its 25px (20px) / 39px (30px) line boxes put the
//! text spans and the bars below them where Chrome's boxes are (control:
//! fixtures/screenshot_web_fonts_chrome156.json, collector
//! fixture-gen/screenshot-web-fonts-chrome.mjs). A localhost server, nothing else.
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_mcp::{call_tool, Session};

const PAGE: &[u8] = include_bytes!("fixtures/web_fonts/page.html");
const SHEET: &[u8] = include_bytes!("fixtures/web_fonts/fonts.css");
const WOFF2: &[u8] = include_bytes!(
    "../../../../harness/detection/fixtures/fonts/ibm-plex-mono/IBMPlexMono-Regular.woff2"
);
const TTF: &[u8] = include_bytes!(
    "../../../../harness/detection/fixtures/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf"
);
const CONTROL: &str = include_str!("fixtures/screenshot_web_fonts_chrome156.json");

fn file(path: &str) -> Option<(&'static str, &'static [u8])> {
    match path {
        "/" => Some(("text/html", PAGE)),
        "/css/fonts.css" => Some(("text/css", SHEET)),
        "/fonts/plex.woff2" => Some(("font/woff2", WOFF2)),
        "/fonts/plex.ttf" => Some(("font/ttf", TTF)),
        _ => None,
    }
}

type Requests = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

async fn serve(requests: Requests) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let requests = requests.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 16384];
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                requests.lock().unwrap().push(path.clone());
                let (status, kind, body) = match file(&path) {
                    Some((kind, body)) => ("200 OK", kind, body),
                    None => ("404 Not Found", "text/plain", &b""[..]),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(body).await;
                let _ = socket.flush().await;
            });
        }
    });
    base
}

struct Shot {
    width: u32,
    rgba: Vec<u8>,
}

impl Shot {
    fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let at = ((y * self.width + x) * 4) as usize;
        [self.rgba[at], self.rgba[at + 1], self.rgba[at + 2]]
    }
    /// The first row at or below `from` whose pixel at `x` is `color`.
    fn top(&self, x: u32, from: u32, color: [u8; 3]) -> Option<u32> {
        (from..600).find(|&y| self.pixel(x, y) == color)
    }
    /// Each span's painted width and the top of the bar below it.
    fn boxes(&self) -> Value {
        json!({
            "m": [self.run(12, BLUE), self.top(400, 0, RED)],
            "n": [self.run(55, BLUE), self.top(400, 36, RED)],
        })
    }
    /// How far the run of `color` reaches along row `y` from x = 0.
    fn run(&self, y: u32, color: [u8; 3]) -> u32 {
        (0..self.width)
            .take_while(|&x| self.pixel(x, y) == color)
            .count() as u32
    }
}

const BLUE: [u8; 3] = [0, 0, 255];
const RED: [u8; 3] = [255, 0, 0];

#[tokio::test(flavor = "multi_thread")]
async fn static_screenshots_lay_out_and_paint_the_pages_web_fonts_like_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    assert_eq!(control["errors"], json!([]));
    for (path, sha) in control["filesSha256"].as_object().unwrap() {
        let (_, body) = file(path).unwrap();
        assert_eq!(*sha, format!("{:x}", Sha256::digest(body)), "{path}");
    }
    let chrome = &control["result"];
    assert_eq!(
        chrome["fonts"],
        json!([["Plex", "loaded"], ["PlexInline", "loaded"]])
    );
    let requests = Requests::default();
    let base = serve(requests.clone()).await;
    let mut session = Session::new();
    call_tool(&mut session, "goto", &json!({ "url": format!("{base}/") }))
        .await
        .unwrap();
    let shot = screenshot(&mut session, json!({ "width": 800, "height": 600 })).await;
    let number = |value: &Value| value.as_f64().unwrap() as u32;
    // Both faces were fetched (the sheet's second source, the inline sheet's own).
    let requests = requests.lock().unwrap().clone();
    for path in [
        "/fonts/missing.woff2",
        "/fonts/plex.woff2",
        "/fonts/plex.ttf",
    ] {
        assert!(requests.iter().any(|seen| seen == path), "{requests:?}");
    }
    let expected = json!({
        "m": [number(&chrome["m"][2]), number(&chrome["bar"])],
        "n": [number(&chrome["n"][2]), number(&chrome["bar2"])],
    });
    assert_eq!(shot.boxes(), expected);
    // Without the faces (`fonts: false`) the fallback face lays the text out otherwise.
    let bare = screenshot(
        &mut session,
        json!({ "width": 800, "height": 600, "fonts": false }),
    )
    .await;
    assert_ne!(bare.boxes(), expected);
}

async fn screenshot(session: &mut Session, args: Value) -> Shot {
    let shot = call_tool(session, "screenshot", &args).await.unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode(shot["base64"].as_str().unwrap())
        .unwrap();
    let image = turbo_dom_layout::image::decode_png_rgba(&png).unwrap();
    Shot {
        width: image.width,
        rgba: image.rgba,
    }
}
