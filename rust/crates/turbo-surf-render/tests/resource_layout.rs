use sha2::{Digest, Sha256};
use turbo_surf_render::run_with_dom;

#[test]
fn only_unused_definitions_leave_real_chrome_geometry_unchanged() {
    let source = include_str!("../../../../harness/detection/resource-definitions-probe.js");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/resource-definitions-chrome155.json"
    ))
    .unwrap();
    assert_eq!(expected["browser"], "Chrome/155.0.8059.80");
    assert_eq!(expected["v8"], "15.5.35.23");
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    let probe = source.trim().trim_end_matches(';');
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn referenced_fonts_and_animations_never_silently_fall_back_or_freeze() {
    let cases = [
        ("@font-face{font-family:Used;src:url('/face.woff2')}#target{font-family:env(unknown)}", "cannot prove"),
        ("@keyframes Spin{to{width:100px}}#target{animation-name:env(unknown)}", "cannot prove"),
        ("@font-face{font-family:'Used Face';src:url('/face.woff2')}#target{font-family:'uSeD FaCe'}", "font-face resource"),
        (r#"@font-face{font-family:'Used\20 Face';src:url('/face.woff2')}#target{font:16px 'Used Face'}"#, "font-face resource"),
        ("@font-face{font-family:'Used Face';src:url('/face.woff2')}#target{--f:'Used Face';font-family:var(--f)}", "font-face resource"),
        ("@keyframes Spin{to{width:100px}}#target{animation:Spin 1s}", "animation resource"),
        ("@keyframes 'Spin Name'{to{width:100px}}#target{animation-name:'Spin Name'}", "animation resource"),
    ];
    for (css, diagnostic) in cases {
        let html = format!("<!doctype html><style>#target{{width:43px;height:17px}}{css}</style><div id='target'></div>");
        let actual = run_with_dom(&html, "(()=>{globalThis.getComputedStyle=()=>{throw new Error('public style getter must stay unreachable');};try{document.getElementById('target').getBoundingClientRect();return 'silently admitted';}catch(error){return error.message;}})()").unwrap();
        assert!(actual.contains(diagnostic), "{css}: {actual}");
    }
}

#[test]
fn definition_lifecycle_uses_actual_sheet_state_instead_of_old_layout_cache() {
    let actual = run_with_dom(
        "<!doctype html><style id='resource'>@font-face{font-family:Used;src:url('/face.woff2')}</style><div id='target' style='font-family:Used;width:43px;height:17px'></div>",
        "(()=>{const sheet=document.getElementById('resource').sheet,node=document.getElementById('target');const read=()=>{try{return node.getBoundingClientRect().width;}catch(error){return error.message.includes('font-face resource');}};const result=[read()];sheet.disabled=true;result.push(read());sheet.disabled=false;result.push(read());sheet.deleteRule(0);result.push(read());return JSON.stringify(result);})()",
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!([true, 43, true, 43])
    );
}
