//! Offline end-to-end: an HTML string renders into a valid PNG and a valid SVG.

use turbo_surf_raster::{screenshot_png, screenshot_svg, Format, Viewport};

const PAGE: &str = r#"<html><head>
    <style>.card{background-color:#3366cc;padding:16px} p{color:#ffffff;font-size:24px}</style>
  </head><body>
    <div class="card"><p>Hello Screenshot</p></div>
  </body></html>"#;

#[test]
fn renders_png_with_magic_and_size() {
    let vp = Viewport {
        width: 640,
        height: 200,
    };
    let png = screenshot_png(PAGE, vp).expect("png");
    // PNG signature.
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    // IHDR carries width/height big-endian at bytes 16..24.
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    assert_eq!((w, h), (640, 200));
}

#[test]
fn renders_svg_with_boxes_and_glyph_paths() {
    let svg = screenshot_svg(PAGE, Viewport::DEFAULT).expect("svg");
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("</svg>"));
    // The blue card background.
    assert!(svg.contains("#3366cc"), "expected card background rect");
    // Text rendered as outline paths (self-contained, no viewer font).
    assert!(svg.contains("<path d=\"M"), "expected glyph outline paths");
}

#[test]
fn z_index_controls_stacking_paint_order() {
    // Two overlapping absolutely-positioned boxes: the box that appears LATER in
    // the DOM has the LOWER z-index, so it must paint UNDERNEATH. SVG paints in
    // document order (later markup = on top), so in the output the low-z green
    // rect must come before the high-z red rect.
    let page = r#"<html><body style="margin:0">
        <div style="position:absolute;top:0;left:0;width:80px;height:80px;background-color:#ff0000;z-index:2"></div>
        <div style="position:absolute;top:0;left:0;width:80px;height:80px;background-color:#00aa00;z-index:1"></div>
      </body></html>"#;
    let svg = screenshot_svg(page, Viewport::DEFAULT).expect("svg");
    let green = svg.find("#00aa00").expect("green rect present");
    let red = svg.find("#ff0000").expect("red rect present");
    assert!(green < red, "z:1 (green) must paint before z:2 (red)");
}

#[test]
fn format_dispatch_matches_direct() {
    let vp = Viewport {
        width: 320,
        height: 240,
    };
    let via_enum = turbo_surf_raster::screenshot(PAGE, vp, Format::Png).expect("png");
    assert_eq!(&via_enum[..4], &[0x89, b'P', b'N', b'G']);
}

#[test]
fn default_viewport_is_1280x800() {
    assert_eq!(
        Viewport::default(),
        Viewport {
            width: 1280,
            height: 800
        }
    );
}
