//! Offline end-to-end: an HTML string renders into a valid PNG and a valid SVG.

use std::collections::HashMap;

use turbo_surf_raster::{
    image_urls, screenshot_png, screenshot_png_with_assets, screenshot_svg,
    screenshot_svg_with_assets, Format, ImageAssets, Viewport,
};

/// A `w × h` solid-colour RGBA PNG fixture (built with tiny-skia so the test
/// needs no committed binary).
fn solid_png(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    pm.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
    pm.encode_png().unwrap()
}

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
fn image_urls_lists_img_and_background_refs() {
    let html = r#"<img src="/logo.png"><div style="background-image:url(bg.jpg)"></div>"#;
    assert_eq!(image_urls(html), vec!["/logo.png", "bg.jpg"]);
}

#[test]
fn png_paints_supplied_image_bytes_not_a_placeholder() {
    // A 40×40 red `<img>` whose bytes the caller supplies must render as red
    // pixels (the fixture colour), not the grey placeholder or the white canvas.
    let mut assets: ImageAssets = HashMap::new();
    assets.insert("r.png".to_string(), solid_png(4, 4, 255, 0, 0));
    let page = r#"<html><body style="margin:0">
        <img src="r.png" style="width:40px;height:40px">
      </body></html>"#;
    let vp = Viewport {
        width: 100,
        height: 100,
    };
    let png = screenshot_png_with_assets(page, "", vp, &assets, false).expect("png");
    let pm = tiny_skia::Pixmap::decode_png(&png).expect("decode output");
    // Sample the middle of the image box (~20,20).
    let px = pm.pixel(20, 20).expect("pixel");
    assert!(
        px.red() > 200 && px.green() < 60 && px.blue() < 60,
        "image box should be red, got ({},{},{})",
        px.red(),
        px.green(),
        px.blue()
    );
}

#[test]
fn png_falls_back_to_placeholder_without_bytes() {
    // Same page, no supplied bytes: the `<img>` box lays out but paints nothing
    // (no Image fragment), so the middle stays the white canvas — definitely not
    // red.
    let page = r#"<html><body style="margin:0">
        <img src="r.png" style="width:40px;height:40px">
      </body></html>"#;
    let vp = Viewport {
        width: 100,
        height: 100,
    };
    let png = screenshot_png_with_assets(page, "", vp, &ImageAssets::new(), false).expect("png");
    let pm = tiny_skia::Pixmap::decode_png(&png).expect("decode");
    let px = pm.pixel(20, 20).expect("pixel");
    assert!(
        px.red() > 200 && px.green() > 200 && px.blue() > 200,
        "no image → canvas stays light"
    );
}

#[test]
fn full_page_grows_height_to_content() {
    // Content taller than the viewport: a viewport-clipped shot is exactly the
    // viewport height, while `full_page` grows to fit the whole content.
    let page = r#"<html><body style="margin:0">
        <div style="height:2000px;background:#eee"></div>
      </body></html>"#;
    let vp = Viewport {
        width: 400,
        height: 300,
    };
    let clipped =
        screenshot_png_with_assets(page, "", vp, &ImageAssets::new(), false).expect("png");
    let full = screenshot_png_with_assets(page, "", vp, &ImageAssets::new(), true).expect("png");
    let hc = u32::from_be_bytes([clipped[20], clipped[21], clipped[22], clipped[23]]);
    let hf = u32::from_be_bytes([full[20], full[21], full[22], full[23]]);
    let wf = u32::from_be_bytes([full[16], full[17], full[18], full[19]]);
    assert_eq!(hc, 300, "clipped keeps the viewport height");
    assert_eq!(wf, 400, "full page keeps the viewport width");
    assert!(
        hf >= 2000,
        "full page grows to the content height, got {hf}"
    );
}

#[test]
fn svg_source_image_is_rasterized_and_painted() {
    // An `<img>` whose bytes are an SVG must be rasterized (resvg) and painted —
    // proving the normalize-any-format-to-RGBA path sizes + paints non-PNG/JPEG
    // sources end-to-end (turbo-html2pdf only ever sees the re-encoded PNG).
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="#0000ff"/></svg>"##;
    let mut assets: ImageAssets = HashMap::new();
    assets.insert("logo.svg".to_string(), svg.to_vec());
    let page = r#"<html><body style="margin:0">
        <img src="logo.svg" style="width:40px;height:40px">
      </body></html>"#;
    let vp = Viewport {
        width: 100,
        height: 100,
    };
    let png = screenshot_png_with_assets(page, "", vp, &assets, false).expect("png");
    let pm = tiny_skia::Pixmap::decode_png(&png).expect("decode");
    let px = pm.pixel(20, 20).expect("pixel");
    assert!(
        px.blue() > 200 && px.red() < 60 && px.green() < 60,
        "SVG box should be blue, got ({},{},{})",
        px.red(),
        px.green(),
        px.blue()
    );
}

#[test]
fn svg_embeds_supplied_image_as_data_uri() {
    let mut assets: ImageAssets = HashMap::new();
    assets.insert("r.png".to_string(), solid_png(2, 2, 0, 128, 0));
    let page = r#"<body style="margin:0"><img src="r.png" style="width:30px;height:30px"></body>"#;
    let svg = screenshot_svg_with_assets(page, "", Viewport::DEFAULT, &assets, false).expect("svg");
    assert!(svg.contains("<image "), "expected an <image> element");
    assert!(
        svg.contains("href=\"data:image/png;base64,"),
        "expected a base64 PNG data URI"
    );
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

#[test]
fn background_image_div_paints_supplied_bytes() {
    let mut assets: ImageAssets = HashMap::new();
    assets.insert("r.png".to_string(), solid_png(4, 4, 255, 0, 0));
    let page = r#"<body style="margin:0"><div style="width:60px;height:60px;background-image:url(r.png)"></div></body>"#;
    let svg = screenshot_svg_with_assets(page, "", Viewport::DEFAULT, &assets, false).expect("svg");
    assert!(
        svg.contains("<image "),
        "background-image should paint an <image>, svg=\n{svg}"
    );
}

#[test]
fn system_fonts_opt_in_renders() {
    // The opt-in system-font path produces a valid PNG (fonts resolve against the
    // machine's installed set; falls back to bundled when absent).
    use turbo_surf_raster::screenshot_png_with_opts;
    let vp = Viewport {
        width: 200,
        height: 100,
    };
    let png = screenshot_png_with_opts(
        "<body style='font-family:Arial'>hi</body>",
        "",
        vp,
        &ImageAssets::new(),
        false,
        true,
    )
    .expect("png");
    assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
}

#[test]
fn image_urls_extracts_lazy_and_responsive_attrs() {
    // Modern sites omit `src` and put the real URL in a lazy/responsive attribute
    // (Nike uses `data-landscape-url`). `image_urls` must recover all of them.
    use turbo_surf_raster::image_urls;
    let html = concat!(
        r#"<img src="a.png">"#,
        r#"<img data-src="b.png">"#,
        r#"<img srcset="c.png 1x, c2.png 2x">"#,
        r#"<img data-landscape-url="d.png">"#,
        r#"<img src="" data-original="e.png">"#,
    );
    let urls = image_urls(html);
    for want in ["a.png", "b.png", "c.png", "d.png", "e.png"] {
        assert!(urls.iter().any(|u| u == want), "missing {want} in {urls:?}");
    }
}

#[test]
fn delazy_populates_missing_img_src_for_layout() {
    // The layout reads `<img src>`; a `src`-less lazy image must get its `src`
    // filled from the lazy attr so an image box (and its pixels) render.
    use turbo_surf_raster::delazy_images;
    let out = delazy_images(r#"<img data-landscape-url="hero.jpg" alt="x">"#);
    assert!(
        out.contains(r#"src="hero.jpg""#),
        "src injected, got: {out}"
    );
}

#[test]
fn border_radius_renders_rounded_rect() {
    // A `border-radius:50%` box (the Codex radio/checkbox circle) must paint round,
    // not square. In SVG that surfaces as an `rx`/`ry` on the background `<rect>`;
    // the PNG path draws the same rounded corners via tiny-skia.
    let page = r#"<html><body>
        <div style="width:18px;height:18px;border-radius:50%;background-color:#3366cc"></div>
      </body></html>"#;
    let svg = screenshot_svg(page, Viewport::DEFAULT).expect("svg");
    assert!(svg.contains("#3366cc"), "circle background present");
    assert!(svg.contains("rx="), "border-radius emits a rounded rect (rx)");
    // PNG still renders (rounded fill path is valid).
    let png = screenshot_png(page, Viewport::DEFAULT).expect("png");
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
}
