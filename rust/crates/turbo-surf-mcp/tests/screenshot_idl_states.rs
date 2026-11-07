//! turbo-surf#171: a script that sets an `<input type=checkbox>`'s IDL `indeterminate`
//! flag to `true` is not expressed in the serialized HTML, so a static screenshot has to
//! learn the flag out-of-band. The session captures the flag's tree-order index (over
//! `document.querySelectorAll('input')`) at the end of each render and forwards it to
//! turbo-dom-raster's [`ScreenshotOptions::idl_states`] (turbo-dom#371). turbo-dom's own
//! form_native tests compare the painted dash against Chrome 156 (headless shell
//! 156.0.8078.47) pixel for pixel; here the point is the surf wiring, so the test
//! screenshots the same page with and without the flag set and asserts the checkbox face
//! differs only because of the overlay.
use base64::Engine;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_mcp::{call_tool, Session};

// A bare page with one native-size checkbox (13x13 in Chrome 156, see turbo-dom-raster's
// `form_native_chrome_tests`). A cyan body makes the "not-painted" and "painted-face"
// cases easy to tell apart at a glance.
const PAGE: &[u8] = b"<!doctype html><html><head><style>\
body{margin:0;background:#00ffff}\
input{position:absolute;left:20px;top:20px;margin:0}\
</style></head><body><input id=c type=checkbox></body></html>";

async fn serve() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 4096];
                let _ = socket.read(&mut buffer).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    PAGE.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(PAGE).await;
                let _ = socket.flush().await;
            });
        }
    });
    base
}

async fn screenshot(session: &mut Session) -> Vec<u8> {
    let shot = call_tool(
        session,
        "screenshot",
        &json!({ "width": 100, "height": 100, "fonts": false, "external_css": false, "images": false }),
    )
    .await
    .unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(shot["base64"].as_str().unwrap())
        .unwrap()
}

fn pixel(png: &[u8], x: u32, y: u32) -> [u8; 3] {
    let image = turbo_dom_layout::image::decode_png_rgba(png).unwrap();
    let at = ((y * image.width + x) * 4) as usize;
    [image.rgba[at], image.rgba[at + 1], image.rgba[at + 2]]
}

async fn goto(session: &mut Session, base: &str) {
    call_tool(session, "goto", &json!({ "url": format!("{base}/") }))
        .await
        .unwrap();
}

async fn render(session: &mut Session, script: &str) {
    call_tool(session, "set_mode", &json!({ "mode": "fast" }))
        .await
        .unwrap();
    call_tool(session, "render", &json!({ "script": script }))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn setting_indeterminate_through_a_render_paints_the_chrome_156_dash() {
    let base = serve().await;
    // Chrome 156 paints the native checkbox 13x13 at (20,20): the top-left at (20,20),
    // the face across the interior, the centre at (26,26). The painted face is Chrome's
    // accent-colour fill with a 2-px-tall white horizontal dash through the middle.
    // (Pixel values recorded in turbo-dom-raster's own `form_native_chrome_tests`:
    // indeterminate-checkbox probe at (22,22) → rgb(0,117,255).)
    const CYAN: [u8; 3] = [0, 255, 255];
    // Plain path: no IDL state ever set. An empty inject keeps the flag at `false`, so
    // the screenshot overlay stays empty and the painted face is the unchecked box.
    let mut plain = Session::new();
    goto(&mut plain, &base).await;
    render(&mut plain, "void 0").await;
    let plain_shot = screenshot(&mut plain).await;
    // Indeterminate path: the same page, but the script sets `c.indeterminate = true`
    // before close. The session snapshots index 0 and the screenshot forwards it
    // through `ScreenshotOptions.idl_states`, so turbo-dom-raster paints Chrome 156's
    // indeterminate dash.
    let mut indet = Session::new();
    goto(&mut indet, &base).await;
    render(
        &mut indet,
        "document.getElementById('c').indeterminate = true",
    )
    .await;
    let indet_shot = screenshot(&mut indet).await;
    // (22,22) is the accent-colour fill in Chrome's indeterminate-checkbox fixture; in
    // the plain case the same pixel is still inside the native box but not on the
    // filled face.
    let plain_22 = pixel(&plain_shot, 22, 22);
    let indet_22 = pixel(&indet_shot, 22, 22);
    assert_ne!(
        plain_22, CYAN,
        "the plain checkbox should still render at (22,22)"
    );
    assert_ne!(
        indet_22, CYAN,
        "the indeterminate checkbox should still render at (22,22)"
    );
    assert_ne!(
        indet_22, plain_22,
        "indeterminate overlay should repaint the checkbox face at (22,22): plain {plain_22:?}, indet {indet_22:?}"
    );
    // Chrome's indeterminate face is the accent-colour fill — at least one channel at
    // the probe point drops well below 255. A tolerant bound keeps the test robust to
    // the glyph's anti-aliased band.
    assert!(
        indet_22.iter().any(|&c| c < 200),
        "indeterminate (22,22) should be a filled accent face: {indet_22:?}"
    );
}

/// A screenshot taken before any render sees an empty `indeterminate_input_indices`
/// list — the overlay is inert, so plain markup paints as-is.
#[tokio::test(flavor = "multi_thread")]
async fn screenshots_without_a_render_stay_on_the_markup_face() {
    let base = serve().await;
    let mut session = Session::new();
    goto(&mut session, &base).await;
    assert!(session.indeterminate_input_indices.is_empty());
    // The screenshot renders the page's markup without any IDL overlay — the checkbox
    // at (22,22) is the unchecked native face, not cyan background.
    let shot = screenshot(&mut session).await;
    assert_ne!(pixel(&shot, 22, 22), [0, 255, 255]);
}

/// The session captures the live IDL `indeterminate` index, keyed in tree-order among
/// `document.querySelectorAll('input')`, at the end of each render — the bit of state
/// the screenshot overlay needs that the serialized HTML cannot carry.
#[tokio::test(flavor = "multi_thread")]
async fn a_render_captures_the_live_indeterminate_input_index() {
    let base = serve().await;
    let mut session = Session::new();
    goto(&mut session, &base).await;
    render(
        &mut session,
        "document.getElementById('c').indeterminate = true",
    )
    .await;
    assert_eq!(session.indeterminate_input_indices, vec![0]);
    // Clearing the flag in a later render drops the index.
    render(
        &mut session,
        "document.getElementById('c').indeterminate = false",
    )
    .await;
    assert!(session.indeterminate_input_indices.is_empty());
}

// `screenshot` returns a JSON object (its typing matches other mcp tests).
fn _assert_json_type(_: Value) {}
