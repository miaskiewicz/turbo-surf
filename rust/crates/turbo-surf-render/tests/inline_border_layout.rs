use turbo_surf_render::run_with_dom;

#[test]
fn inline_decoration_uses_real_text_advances_and_distinguishes_client_and_offset_metrics() {
    let actual=run_with_dom("<!doctype html><html><head></head><body></body></html>",r#"(()=>{
      document.body.innerHTML='<div style="width:1000px;font:16px/24px monospace"><span id="a">abc</span><span id="b">z</span></div>';
      const a=document.getElementById('a'),b=document.getElementById('b');
      const plain=a.getBoundingClientRect(),before=b.getBoundingClientRect();
      a.style='padding:3px 5px;border:2px solid;margin:0 7px';
      const decorated=a.getBoundingClientRect(),after=b.getBoundingClientRect();
      const range=document.createRange();range.selectNodeContents(a);const text=range.getBoundingClientRect();
      return String(decorated.width===plain.width+14&&after.x===before.x+28&&text.width===plain.width&&text.x===plain.x+14&&a.clientWidth===0&&a.clientHeight===0&&a.offsetWidth===Math.round(decorated.width));
    })()"#).unwrap();
    assert_eq!(actual, "true");
}

#[cfg(target_os = "macos")]
#[test]
fn genuine_inline_borders_empty_nodes_nested_fonts_wrapping_and_range_boxes_match_chrome() {
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    let source = include_str!("../../../../harness/detection/inline-border-layout-probe.js");
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/inline-border-layout-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({})", source.trim().trim_end_matches(';')),
    )
    .unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    if actual != expected["result"] {
        eprintln!("INLINE_ACTUAL {}", serde_json::to_string(&actual).unwrap());
    }
    assert_eq!(actual, expected["result"]);
}
