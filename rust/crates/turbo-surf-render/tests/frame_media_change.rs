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
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .contains("Chrome/156."));
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
