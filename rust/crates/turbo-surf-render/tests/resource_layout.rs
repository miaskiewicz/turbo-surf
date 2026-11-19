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
        (
            "@font-face{font-family:Used;src:url('/face.woff2')}#target{font-family:env(unknown)}",
            "cannot prove",
        ),
        (
            "@keyframes Spin{to{width:100px}}#target{animation-name:env(unknown)}",
            "cannot prove",
        ),
        (
            "@keyframes Spin{to{width:100px}}#target{animation:Spin 1s}",
            "animation resource",
        ),
        (
            "@keyframes 'Spin Name'{to{width:100px}}#target{animation-name:'Spin Name'}",
            "animation resource",
        ),
        (
            "@keyframes additive{0%{width:43px}}#target{animation:additive 100s;animation-composition:add}",
            "animation resource",
        ),
        (
            "@keyframes unresolved{0%{width:var(--size)}}#target{--size:43px;animation:unresolved 100s}",
            "animation resource",
        ),
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
        "(()=>{const sheet=document.getElementById('resource').sheet,node=document.getElementById('target');const read=()=>[node.getBoundingClientRect().width,Array.from(document.fonts,face=>face.status).join()];const result=[read()];sheet.disabled=true;result.push(read());sheet.disabled=false;result.push(read());sheet.deleteRule(0);result.push(read());return JSON.stringify(result);})()",
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!([[43, "error"], [43, ""], [43, "error"], [43, ""]])
    );
}

/// A used `@font-face` family lays out with its fallback while the face loads
/// (turbo-surf#130), whatever case, escape or `var()` spelling names it. The
/// document has no fetchable base, so a started relative source fails at once.
#[test]
fn used_web_fonts_lay_out_and_start_loading() {
    for css in [
        "@font-face{font-family:'Used Face';src:url('/face.woff2')}#target{font-family:'uSeD FaCe'}",
        r#"@font-face{font-family:'Used\20 Face';src:url('/face.woff2')}#target{font:16px 'Used Face'}"#,
        "@font-face{font-family:'Used Face';src:url('/face.woff2')}#target{--f:'Used Face';font-family:var(--f)}",
        "@font-face{font-family:'Ünused';src:url('/face.woff2')}#target{font-family:'Other'}",
    ] {
        let html = format!("<!doctype html><style>#target{{width:43px;height:17px}}{css}</style><div id='target'></div>");
        let actual = run_with_dom(&html, "(()=>{const before=Array.from(document.fonts,face=>face.status).join();const width=document.getElementById('target').getBoundingClientRect().width;return JSON.stringify([before,width,Array.from(document.fonts,face=>face.status).join()]);})()").unwrap();
        let used = !css.contains("Ünused");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
            serde_json::json!(["unloaded", 43, if used { "error" } else { "unloaded" }]),
            "{css}"
        );
    }
}

#[test]
fn opacity_only_animation_does_not_reject_unrelated_geometry() {
    for duration in ["100s", "1ms"] {
        let html = format!("<!doctype html><style>@keyframes fade{{from{{opacity:0}}to{{opacity:1}}}}#sibling{{animation:fade {duration} both}}#target{{width:120px;height:20px}}</style><div id='sibling'>sibling</div><div id='target'>target</div>");
        assert_eq!(
            run_with_dom(
                &html,
                "String(document.getElementById('target').getBoundingClientRect().width)"
            )
            .unwrap(),
            "120"
        );
    }
}

#[test]
fn used_constant_keyframes_are_proven_against_the_actual_underlying_style() {
    let html = "<!doctype html><style>@keyframes constant-layout{0%{display:block}} input{display:block;animation:constant-layout 10ms}#target{width:43px;height:17px}</style><input id=field><div id=target></div>";
    let actual = run_with_dom(html, "JSON.stringify([getComputedStyle(document.getElementById('field')).display,document.getElementById('target').getBoundingClientRect().width])").unwrap();
    assert_eq!(actual, "[\"block\",43]");
}

#[test]
fn constant_animation_admission_tracks_underlying_style_mutations() {
    let html = "<!doctype html><style>@keyframes steady{0%{width:43px}}#target{display:block;width:43px;height:17px;animation:steady 100s}</style><div id=target></div>";
    let actual = run_with_dom(html, "(()=>{const node=document.getElementById('target');const width=node.getBoundingClientRect().width;node.style.width='44px';try{node.getBoundingClientRect();return 'silently frozen';}catch(error){return JSON.stringify([width,error.message.includes('animation resource')]);}})()").unwrap();
    assert_eq!(actual, "[43,true]");
}

#[test]
fn unrelated_rect_is_independent_of_a_descendant_rotation() {
    let html = "<!doctype html><style>@keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}}#spinner{width:40px;height:40px;animation:spin 1.4s linear infinite}#target{width:80px;height:60px;transition-duration:1s}</style><div id=target><span id=spinner></span></div>";
    let actual = run_with_dom(html, "(()=>{const target=document.getElementById('target'),spinner=document.getElementById('spinner');const read=node=>{try{return node.getBoundingClientRect().width}catch(e){return e.message}};const before=read(target);const animated=read(spinner);target.style.animation='spin 1s infinite';const after=read(target);return JSON.stringify([getComputedStyle(spinner).transitionDuration,before,animated,after])})()").unwrap();
    let result: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(result[0], "0s");
    assert_eq!(result[1], 80);
    assert!(result[2].as_str().unwrap().contains("animation resource"));
    assert!(result[3].as_str().unwrap().contains("animation resource"));
}

#[test]
fn rect_animation_admission_keeps_gutter_and_flow_dependencies_strict() {
    for (frame, overflow) in [
        (
            "transform:rotate(90deg)",
            "overflow:auto;scrollbar-gutter:stable",
        ),
        ("width:80px", "overflow:visible"),
    ] {
        let html=format!("<!doctype html><style>@keyframes change{{to{{{frame}}}}}#outer{{{overflow};width:80px;height:60px}}#spin{{animation:change 1s infinite}}</style><div id=outer><div id=spin>spinner</div></div>");
        let result=run_with_dom(&html,"(()=>{try{return document.getElementById('outer').getBoundingClientRect().width}catch(e){return e.message}})()").unwrap();
        assert!(result.contains("animation resource"), "{frame}: {result}");
    }
}

#[test]
fn local_box_admission_cannot_escape_to_root_extent_hit_test_or_reparented_geometry() {
    let html="<!doctype html><style>@keyframes spin{to{transform:rotate(360deg)}}#spin{animation:spin 1s infinite}#target{width:80px;height:60px}</style><div id=target></div><div id=spin></div>";
    let actual=run_with_dom(html,"(()=>{const target=document.getElementById('target'),spin=document.getElementById('spin');const read=f=>{try{return f()}catch(e){return e.message}};const result=[read(()=>target.getBoundingClientRect().width),read(()=>document.documentElement.scrollWidth),read(()=>document.elementFromPoint(0,0))];spin.append(target);result.push(read(()=>target.getBoundingClientRect().width));return JSON.stringify(result)})()").unwrap();
    let result: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(result[0], 80);
    for i in 1..4 {
        assert!(result[i].as_str().unwrap().contains("animation resource"));
    }
}

#[test]
fn rect_animation_admission_remains_strict_after_nonzero_scroll() {
    let html="<!doctype html><style id=sheet>body{height:4000px}#target{width:80px;height:60px}</style><div id=target></div><div id=spin></div>";
    let result=run_with_dom(html,"(()=>{window.scrollTo(0,100);const before=window.scrollY;const sheet=document.getElementById('sheet').sheet;sheet.insertRule('@keyframes spin{to{transform:rotate(360deg)}}');sheet.insertRule('#spin{animation:spin 1s infinite}');try{return JSON.stringify([before,document.getElementById('target').getBoundingClientRect().width])}catch(e){return JSON.stringify([before,e.message])}})()").unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result[0], 100);
    assert!(result[1].as_str().unwrap().contains("animation resource"));
}

#[test]
fn next_scroll_boundary_sequence_reads_known_viewport_extents() {
    for (doctype, viewport_owner) in [
        ("<!doctype html>", "document.documentElement"),
        ("", "document.body"),
    ] {
        let html=format!("{doctype}<style>@keyframes spin{{to{{transform:rotate(360deg)}}}}#spin{{animation:spin 1s infinite}}#target{{width:80px;height:60px}}</style><div id=target><div id=spin></div></div>");
        let result=run_with_dom(&html,&format!("(()=>{{const target=document.getElementById('target'),viewport={viewport_owner};return JSON.stringify([target.getBoundingClientRect().width,viewport.clientWidth,viewport.clientHeight,target.getBoundingClientRect().width,window.innerWidth,window.innerHeight])}})()")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result[0], 80);
        assert_eq!(result[1], result[4]);
        assert_eq!(result[2], result[5]);
        assert_eq!(result[3], 80);
    }
}

#[test]
fn typed_vendor_transform_aliases_preserve_rectangle_independence() {
    let html="<!doctype html><style>@-webkit-keyframes spin{0%{-ms-transform:rotate(0deg);transform:rotate(0deg)}100%{-ms-transform:rotate(360deg);transform:rotate(360deg)}}@keyframes spin{0%{-ms-transform:rotate(0deg);transform:rotate(0deg)}100%{-ms-transform:rotate(360deg);transform:rotate(360deg)}}#spin{display:inline-block;animation:1.4s linear 0s infinite normal none running spin}#target{width:80px;height:60px}</style><div id=target><span id=spin></span></div>";
    let actual=run_with_dom(html,"(()=>{const target=document.getElementById('target'),spin=document.getElementById('spin');const read=node=>{try{return node.getBoundingClientRect().width}catch(e){return e.message}};return JSON.stringify([read(target),document.documentElement.clientHeight,read(spin)])})()").unwrap();
    let result: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(result[0], 80);
    assert!(result[1].as_i64().unwrap() > 0);
    assert!(result[2].as_str().unwrap().contains("animation resource"));
}

#[test]
fn prefixed_opacity_scale_pulse_keeps_independent_rect_but_not_animated_rect() {
    let html="<!doctype html><style>@keyframes pulse{0%,60%,100%{opacity:.3;-ms-transform:scale(.8);transform:scale(.8)}30%{opacity:1;-ms-transform:scale(1);transform:scale(1)}}#pulse{width:8px;height:8px;animation:pulse 1.4s ease-in-out infinite;animation-delay:.2s}#target{width:80px;height:60px}</style><div id=target><div id=pulse></div></div>";
    let actual=run_with_dom(html,"(()=>{const read=node=>{try{return node.getBoundingClientRect().width}catch(e){return e.message}};return JSON.stringify([read(document.getElementById('target')),read(document.getElementById('pulse'))])})()").unwrap();
    let result: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(result[0], 80);
    assert!(result[1].as_str().unwrap().contains("animation resource"));
}

#[test]
fn rotating_svg_spinner_and_dash_circle_leave_parent_rect_independent() {
    let html = CIRCULAR_SPINNER;
    let actual = run_with_dom(html,"(()=>{const read=node=>{try{return node.getBoundingClientRect().width}catch(e){return e.message}};return JSON.stringify([read(document.getElementById('target')),document.documentElement.clientHeight,document.getElementById('target').getBoundingClientRect().height,read(document.getElementById('spin')),(()=>{document.getElementById('spin').style.animation='none';return read(document.querySelector('circle'))})()])})()").unwrap();
    let result: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(result[0], 80);
    assert!(result[1].as_i64().unwrap() > 0);
    assert_eq!(result[2], 60);
    assert!(result[3].as_str().unwrap().contains("animation resource"));
    assert!(result[4].as_str().unwrap().contains("animation resource"));
}

#[test]
fn stroke_dash_animation_cannot_admit_mixed_flow_properties() {
    let html="<!doctype html><style>@keyframes dash{to{stroke-dashoffset:-15px;width:120px}}circle{animation:dash 1s infinite}#target{width:80px;height:60px}</style><div id=target><svg><circle cx=44 cy=44 r=20></circle></svg></div>";
    let actual=run_with_dom(html,"(()=>{try{return document.getElementById('target').getBoundingClientRect().width}catch(e){return e.message}})()").unwrap();
    assert!(actual.contains("animation resource"));
}

const CIRCULAR_SPINNER: &str = "<!doctype html><style>@-webkit-keyframes spin{0%{-ms-transform:rotate(0deg);transform:rotate(0deg)}100%{-ms-transform:rotate(360deg);transform:rotate(360deg)}}@keyframes spin{0%{-ms-transform:rotate(0deg);transform:rotate(0deg)}100%{-ms-transform:rotate(360deg);transform:rotate(360deg)}}@-webkit-keyframes dash{0%{stroke-dasharray:1px,200px;stroke-dashoffset:0px}50%{stroke-dasharray:100px,200px;stroke-dashoffset:-15px}100%{stroke-dasharray:1px,200px;stroke-dashoffset:-126px}}@keyframes dash{0%{stroke-dasharray:1px,200px;stroke-dashoffset:0px}50%{stroke-dasharray:100px,200px;stroke-dashoffset:-15px}100%{stroke-dasharray:1px,200px;stroke-dashoffset:-126px}}#target{display:flex;flex-direction:column;width:80px;height:60px}#spin{display:inline-block;width:40px;height:40px;animation:spin 1.4s linear infinite}svg{display:block;overflow:visible}circle{stroke:currentColor;stroke-dasharray:80px,200px;stroke-dashoffset:0px;animation:dash 1.4s ease-in-out infinite;overflow:visible}</style><div id=target><span id=spin><svg viewBox='22 22 44 44'><circle cx=44 cy=44 r=20.2 fill=none stroke-width=3.6></circle></svg></span></div>";

fn clipped_spinner(overflow: &str) -> String {
    CIRCULAR_SPINNER
        .replace(
            "<span id=spin>",
            &format!("<div style='display:grid'><div id=clip style='{overflow}'><span id=spin>"),
        )
        .replace("</span></div>", "</span></div></div></div>")
}

#[test]
fn rect_outside_hidden_animated_descendant_keeps_its_auto_height() {
    let html = clipped_spinner("overflow:hidden")
        .replace("<div id=target>", "<div id=target style='height:auto'>");
    let actual=run_with_dom(&html,"JSON.stringify([document.getElementById('target').getBoundingClientRect().width,document.getElementById('target').getBoundingClientRect().height,document.documentElement.clientHeight])").unwrap();
    let result: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(result[0], 80);
    assert_eq!(result[1], 44);
    assert!(result[2].as_i64().unwrap() > 0);
}

#[test]
fn clipped_box_admission_keeps_descendant_extent_and_scroll_offsets_strict() {
    let read="(()=>{try{return document.getElementById('target').getBoundingClientRect().width}catch(e){return e.message}})()";
    for overflow in [
        "overflow:auto",
        "overflow:scroll",
        "overflow-x:hidden;overflow-y:visible",
    ] {
        let actual = run_with_dom(&clipped_spinner(overflow), read).unwrap();
        assert_eq!(actual, "80", "{overflow}: {actual}");
        let extent=run_with_dom(&clipped_spinner(overflow),"(()=>{const target=document.getElementById('target');target.getBoundingClientRect();try{return target.scrollHeight}catch(e){return e.message}})()").unwrap();
        assert!(extent.contains("animation resource"), "{extent}");
    }
    let shared = clipped_spinner("overflow:hidden")
        .replace("</style>", "</style><body style='overflow:hidden'>");
    assert_eq!(run_with_dom(&shared, read).unwrap(), "80");
    let actual=run_with_dom(&clipped_spinner("overflow:hidden"),"(()=>{const spin=document.getElementById('spin'),circle=document.querySelector('circle');spin.style.animation='none';circle.style.animation='none';document.body.style.height='4000px';window.scrollTo(0,100);const before=window.scrollY;spin.style.removeProperty('animation');circle.style.removeProperty('animation');try{return JSON.stringify([before,document.getElementById('target').getBoundingClientRect().width])}catch(e){return JSON.stringify([before,e.message])}finally{spin.style.animation='none';circle.style.animation='none';window.scrollTo(0,0);document.body.style.removeProperty('height');spin.style.removeProperty('animation');circle.style.removeProperty('animation')}})()").unwrap();
    let result: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(result[0], 100);
    assert!(result[1].as_str().unwrap().contains("animation resource"));
    let query=run_with_dom(&clipped_spinner("overflow:hidden"),"(()=>{try{return document.getElementById('clip').getBoundingClientRect().width}catch(e){return e.message}})()").unwrap();
    assert_eq!(query, "80");
    let extent=run_with_dom(&clipped_spinner("overflow:hidden"),"(()=>{const clip=document.getElementById('clip');clip.getBoundingClientRect();const name=getComputedStyle(document.getElementById('spin')).animationName;if(name!=='spin')return 'fixture animation missing:'+name;try{return clip.scrollWidth}catch(e){return e.message}})()").unwrap();
    assert!(extent.contains("animation resource"), "{extent}");
}
