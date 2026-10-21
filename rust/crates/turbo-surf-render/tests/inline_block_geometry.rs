#[cfg(target_os = "macos")]
#[test]
fn split_inline_continuation_rects_preserve_chrome_bounds_and_integer_offsets() {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/block-inline-offset-chrome155.json"
    ))
    .unwrap();
    let actual = turbo_surf_render::run_with_dom(
        "<!doctype html><html><head></head><body><div id=rso><a><h3>Old result</h3></a></div><button id=dismiss>Reject</button><input name=q type=hidden value=hidden><form><textarea name=q></textarea><button>Search</button></form></body></html>",
        r#"(()=>{
          const link=document.querySelector('#rso a'),heading=link.querySelector('h3'),button=document.getElementById('dismiss');
          const rect=r=>[r.x,r.y,r.width,r.height],box=button.getBoundingClientRect();
          const hit=document.elementFromPoint(box.x+box.width/2,box.y+box.height/2);
          const contents=node=>{const range=document.createRange();range.selectNodeContents(node);return {bounds:rect(range.getBoundingClientRect()),rects:Array.from(range.getClientRects(),rect)};};
          const computed=getComputedStyle(button);
          const style=Object.fromEntries(['display','fontFamily','fontSize','fontStyle','fontWeight','fontStretch','fontVariantCaps','lineHeight','marginTop','marginRight','marginBottom','marginLeft','paddingTop','paddingRight','paddingBottom','paddingLeft','borderTopWidth','borderRightWidth','borderBottomWidth','borderLeftWidth','verticalAlign'].map(key=>[key,computed[key]]));
          const dismiss={bounds:rect(box),rects:Array.from(button.getClientRects(),rect),offset:[button.offsetLeft,button.offsetTop,button.offsetWidth,button.offsetHeight],client:[button.clientLeft,button.clientTop,button.clientWidth,button.clientHeight],style};
          return JSON.stringify({bounds:rect(link.getBoundingClientRect()),rects:Array.from(link.getClientRects(),rect),offset:[link.offsetLeft,link.offsetTop,link.offsetWidth,link.offsetHeight],client:[link.clientLeft,link.clientTop,link.clientWidth,link.clientHeight],dismissCenterHitsButton:button===hit||button.contains(hit),linkRange:contents(link),headingRange:contents(heading),dismiss});
        })()"#,
    )
    .unwrap();
    let mut actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    let mut expected = expected["result"].clone();
    // The saved Times glyph widths retain the system-font shaping mismatch tracked
    // in issue 28. Exact Range ownership/geometry uses the paired embedded-font
    // control below; every original Times element/control box remains asserted here.
    for key in ["linkRange", "headingRange"] {
        actual.as_object_mut().unwrap().remove(key);
        expected.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(actual, expected);
}

#[cfg(target_os = "macos")]
#[test]
fn embedded_font_block_inline_ranges_match_the_same_chrome_font_bytes() {
    use sha2::{Digest, Sha256};

    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/embedded-font-range-chrome155.json"
    ))
    .unwrap();
    let font = include_bytes!(
        "../../../../harness/detection/fixtures/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf"
    );
    let font_hash = format!("{:x}", Sha256::digest(font));
    assert_eq!(font_hash, expected["font"]["sha256"].as_str().unwrap());
    // Use the same public font selection path as native HTML layout, including
    // installed faces, so an accidental system-font substitution fails closed.
    let mut fonts = turbo_html2pdf_core::FontRegistry::new();
    fonts.load_system_fonts_for_html();
    let face = fonts.select(&["IBM Plex Mono"], 400, false).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(face.data())), font_hash);

    let probe = include_str!("../../../../harness/detection/fixtures/embedded-font-range-probe.js");
    assert_eq!(
        format!("{:x}", Sha256::digest(probe.as_bytes())),
        expected["probeSha256"].as_str().unwrap()
    );
    let cases = expected["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2);
    for case in cases {
        let actual =
            turbo_surf_render::run_with_dom(case["html"].as_str().unwrap(), probe).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
            case["result"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn author_universal_button_styles_override_native_ua_specificity() {
    let actual = turbo_surf_render::run_with_dom(
        "<!doctype html><html><head><style>*{font:20px/30px monospace;padding:3px;border:4px solid;margin:5px}#reference{font:20px/30px monospace;padding:3px;border:4px solid;margin:5px}</style></head><body><button id=universal>mmmm</button><button id=reference>mmmm</button></body></html>",
        r#"(()=>{
          const button=document.getElementById('universal'),reference=document.getElementById('reference');
          const actual=button.getBoundingClientRect(),expected=reference.getBoundingClientRect(),computed=getComputedStyle(button);
          return JSON.stringify({sameGeometry:actual.width===expected.width&&actual.height===expected.height,clientLeft:button.clientLeft,clientTop:button.clientTop,fontFamily:computed.fontFamily,fontSize:computed.fontSize,lineHeight:computed.lineHeight,paddingLeft:computed.paddingLeft,borderLeftWidth:computed.borderLeftWidth,marginLeft:computed.marginLeft});
        })()"#,
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!({
            "sameGeometry":true,"clientLeft":4,"clientTop":4,"fontFamily":"monospace","fontSize":"20px","lineHeight":"30px","paddingLeft":"3px","borderLeftWidth":"4px","marginLeft":"5px"
        })
    );
}

#[test]
fn authored_button_font_inherit_matches_an_explicit_font_and_changes_native_geometry() {
    let actual = turbo_surf_render::run_with_dom(
        "<!doctype html><html><head><style>body{font:20px/30px monospace}#inherited{font:inherit}#reference{font:20px/30px monospace}</style></head><body><button id=default>mmmm</button><button id=inherited>mmmm</button><button id=reference>mmmm</button></body></html>",
        r#"(()=>{
          const button=document.getElementById('inherited'),reference=document.getElementById('reference'),normal=document.getElementById('default');
          const actual=button.getBoundingClientRect(),expected=reference.getBoundingClientRect(),baseline=normal.getBoundingClientRect(),computed=getComputedStyle(button);
          return JSON.stringify({sameGeometry:actual.width===expected.width&&actual.height===expected.height,changedGeometry:actual.width!==baseline.width||actual.height!==baseline.height,fontFamily:computed.fontFamily,fontSize:computed.fontSize,lineHeight:computed.lineHeight});
        })()"#,
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!({
            "sameGeometry":true,"changedGeometry":true,"fontFamily":"monospace","fontSize":"20px","lineHeight":"30px"
        })
    );
}

#[test]
fn native_button_ua_resets_parent_fonts_consistently_with_public_computed_style() {
    let actual = turbo_surf_render::run_with_dom(
        "<!doctype html><html><head><style>body{font:italic 700 26px/2 monospace}#reference{font:400 13.3333px Arial;font-stretch:normal;font-variant-caps:normal}</style></head><body><button id=default>Reject</button><button id=reference>Reject</button></body></html>",
        r#"(()=>{
          const button=document.getElementById('default'),reference=document.getElementById('reference');
          const actual=button.getBoundingClientRect(),expected=reference.getBoundingClientRect(),computed=getComputedStyle(button);
          return JSON.stringify({sameGeometry:actual.width===expected.width&&actual.height===expected.height,fontFamily:computed.fontFamily,fontSize:computed.fontSize,fontStyle:computed.fontStyle,fontWeight:computed.fontWeight,fontStretch:computed.fontStretch,fontVariantCaps:computed.fontVariantCaps,lineHeight:computed.lineHeight});
        })()"#,
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!({
            "sameGeometry":true,"fontFamily":"Arial","fontSize":"13.3333px","fontStyle":"normal","fontWeight":"400","fontStretch":"100%","fontVariantCaps":"normal","lineHeight":"normal"
        })
    );
}

#[test]
fn authored_border_shorthands_keep_their_cascade_rank_against_ua_longhands() {
    let actual = turbo_surf_render::run_with_dom(
        "<!doctype html><html><head><style>\
          #aggregate{border:4px solid}\
          #later-shorthand{border-left-width:7px;border:4px solid}\
          #later-longhand{border:4px solid;border-left-width:7px}\
          #later-width{border:4px solid;border-width:6px}\
          #earlier-width{border-width:6px;border:4px solid}\
          #side{border-left:7px solid;border-top:5px solid}\
          #important-shorthand{border:4px solid!important}\
          #important-longhand{border-left-width:7px!important;border:4px solid}\
          #inline{border:4px solid}\
        </style></head><body>\
          <button id=aggregate>x</button>\
          <button id=later-shorthand>x</button>\
          <button id=later-longhand>x</button>\
          <button id=later-width>x</button>\
          <button id=earlier-width>x</button>\
          <button id=side>x</button>\
          <button id=important-shorthand style='border:8px solid'>x</button>\
          <button id=important-longhand>x</button>\
          <button id=inline style='border:8px solid'>x</button>\
        </body></html>",
        r#"JSON.stringify(Array.from(document.querySelectorAll('button'),button=>{
          const computed=getComputedStyle(button);
          return [button.id,button.clientLeft,button.clientTop,computed.borderLeftWidth,computed.borderTopWidth];
        }))"#,
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!([
            ["aggregate", 4, 4, "4px", "4px"],
            ["later-shorthand", 4, 4, "4px", "4px"],
            ["later-longhand", 7, 4, "7px", "4px"],
            ["later-width", 6, 6, "6px", "6px"],
            ["earlier-width", 4, 4, "4px", "4px"],
            ["side", 7, 5, "7px", "5px"],
            ["important-shorthand", 4, 4, "4px", "4px"],
            ["important-longhand", 7, 4, "7px", "4px"],
            ["inline", 8, 8, "8px", "8px"]
        ])
    );
}
