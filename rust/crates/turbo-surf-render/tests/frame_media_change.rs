use sha2::{Digest, Sha256};

/// A child window whose iframe resizes gets a `resize` event in the next rendering update,
/// and its MediaQueryList fires `change` (listener, then `onchange`) when `matches` flips,
/// like Chrome 156 (turbo-dom#287/#288 MediaQueryList semantics).
#[tokio::test(flavor = "current_thread")]
async fn frame_resize_fires_window_resize_and_media_query_change_like_chrome156() {
    let source = include_str!("../../../../harness/detection/frame-media-change-probe.js");
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-media-change-chrome156.json"
    ))
    .unwrap();
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    assert!(control["browser"].as_str().unwrap().contains("Chrome/156."));
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "https://media.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &page
            .eval_json_expression(source.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, control["result"]);
    page.close();
}

/// The binding's JS cascade evaluates `@media` through the window's `matchMedia`, which
/// turbo-surf owns, so `getComputedStyle` follows turbo-surf's viewport: the 1280px top
/// window and a 100px iframe's own viewport, as in Chrome.
#[tokio::test(flavor = "current_thread")]
async fn computed_style_media_follows_the_surf_viewport() {
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "https://media.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let probe = r#"(async () => {
      const css = "<style>@media (min-width: 1000px){p{color:rgb(1,2,3)}}@media (max-width: 999px){p{color:rgb(4,5,6)}}</style><p>x</p>";
      document.body.innerHTML = css;
      const frame = document.createElement("iframe");
      frame.style = "width:100px;height:50px;border:0";
      frame.srcdoc = "<!doctype html>" + css;
      document.body.append(frame);
      await new Promise((resolve) => (frame.onload = resolve));
      const color = (win) => win.getComputedStyle(win.document.querySelector("p")).color;
      return [innerWidth, color(window), frame.contentWindow.innerWidth, color(frame.contentWindow)];
    })()"#;
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    assert_eq!(
        actual,
        serde_json::json!([1280, "rgb(1, 2, 3)", 100, "rgb(4, 5, 6)"])
    );
    page.close();
}
