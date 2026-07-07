//! Vector painter: walk a [`Fragment`] galley into a standalone SVG document.
//! Same stacking-order walk as the raster painter, so the two agree. Glyphs are
//! emitted as filled `<path>` outlines (self-contained — no viewer font needed),
//! boxes/borders as `<rect>`, and images as base64 PNG `data:` URI `<image>`s
//! (any decoded source format; unknown formats fall back to a placeholder rect).

use std::fmt::Write;

use turbo_html2pdf_core::layout::value::{BorderEdges, BorderSide};
use turbo_html2pdf_core::{Fragment, FragmentContent, PositionedGlyph, Rgba};

use crate::glyph::{self, Pen, Tracer};
use crate::image_paint::{self, DecodedAssets};

const IMAGE_PLACEHOLDER: Rgba = Rgba {
    r: 220,
    g: 220,
    b: 220,
    a: 255,
};

/// Paint `galley` into a `width × height` SVG document string over a `bg` canvas
/// fill (the propagated root/body background).
pub fn paint(
    galley: &Fragment,
    width: u32,
    height: u32,
    bg: Rgba,
    images: &DecodedAssets,
) -> String {
    let mut svg = String::with_capacity(4096);
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\">\n<rect width=\"{width}\" height=\"{height}\" {}/>",
        fill_attrs(bg)
    );
    paint_fragment(&mut svg, galley, images);
    svg.push_str("</svg>\n");
    svg
}

fn paint_fragment(svg: &mut String, f: &Fragment, images: &DecodedAssets) {
    match &f.content {
        FragmentContent::Box {
            background,
            border,
            border_radius,
        } => {
            if let Some(bg) = background {
                rect_r(svg, f.x, f.y, f.width, f.height, *bg, *border_radius);
            }
            paint_border(svg, f, border);
        }
        FragmentContent::TextLine {
            glyphs,
            face,
            font_size,
            color,
        } => paint_text(
            svg,
            f,
            glyphs,
            face.data(),
            face.units_per_em(),
            *font_size,
            *color,
        ),
        FragmentContent::Image(placement) => paint_image(svg, f, &placement.name, images),
        FragmentContent::Directive(_) => {}
    }
    // Paint children back-to-front in CSS stacking order (§9.9), not raw DOM
    // order, so `position`/`z-index` boxes (menus, modals) layer correctly.
    for &i in &f.paint_order() {
        paint_fragment(svg, &f.children[i], images);
    }
}

/// Embed an image as a base64 `data:` URI `<image>` filling its layout box; fall
/// back to a neutral placeholder rect when the bytes are absent or unrecognized.
/// `preserveAspectRatio="none"` because layout already sized the box to the
/// image's aspect ratio.
fn paint_image(svg: &mut String, f: &Fragment, name: &str, images: &DecodedAssets) {
    match images.get(name).and_then(image_paint::data_uri) {
        Some(uri) => {
            let _ = writeln!(
                svg,
                "<image x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" \
                 preserveAspectRatio=\"none\" href=\"{uri}\"/>",
                f.x, f.y, f.width, f.height
            );
        }
        None => rect(svg, f.x, f.y, f.width, f.height, IMAGE_PLACEHOLDER),
    }
}

/// `rgba(...)`-free fill: an opaque `#rrggbb` plus a separate `fill-opacity`
/// when the color carries alpha (keeps output ASCII + widely compatible).
fn fill_attrs(c: Rgba) -> String {
    if c.a == 255 {
        format!("fill=\"#{:02x}{:02x}{:02x}\"", c.r, c.g, c.b)
    } else {
        format!(
            "fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{:.3}\"",
            c.r,
            c.g,
            c.b,
            c.a as f32 / 255.0
        )
    }
}

fn rect(svg: &mut String, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
    rect_r(svg, x, y, w, h, c, 0.0);
}

/// A filled `<rect>`, rounded by `r` px (`rx`/`ry`) when `r > 0`.
fn rect_r(svg: &mut String, x: f32, y: f32, w: f32, h: f32, c: Rgba, r: f32) {
    if w <= 0.0 || h <= 0.0 || c.a == 0 {
        return;
    }
    let radius = if r > 0.5 {
        format!(" rx=\"{r:.2}\" ry=\"{r:.2}\"")
    } else {
        String::new()
    };
    let _ = writeln!(
        svg,
        "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\"{radius} {}/>",
        fill_attrs(c)
    );
}

fn paint_border(svg: &mut String, f: &Fragment, b: &BorderEdges) {
    let side = |s: &BorderSide| s.color.filter(|_| s.width > 0).map(|c| (s.width as f32, c));
    if let Some((w, c)) = side(&b.top) {
        rect(svg, f.x, f.y, f.width, w, c);
    }
    if let Some((w, c)) = side(&b.bottom) {
        rect(svg, f.x, f.y + f.height - w, f.width, w, c);
    }
    if let Some((w, c)) = side(&b.left) {
        rect(svg, f.x, f.y, w, f.height, c);
    }
    if let Some((w, c)) = side(&b.right) {
        rect(svg, f.x + f.width - w, f.y, w, f.height, c);
    }
}

/// An SVG path-data sink for [`glyph::trace_glyph`].
#[derive(Default)]
struct PathSink(String);

impl Tracer for PathSink {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "M{x:.2} {y:.2} ");
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "L{x:.2} {y:.2} ");
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let _ = write!(self.0, "Q{cx:.2} {cy:.2} {x:.2} {y:.2} ");
    }
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let _ = write!(
            self.0,
            "C{c1x:.2} {c1y:.2} {c2x:.2} {c2y:.2} {x:.2} {y:.2} "
        );
    }
    fn close(&mut self) {
        self.0.push_str("Z ");
    }
}

fn paint_text(
    svg: &mut String,
    f: &Fragment,
    glyphs: &[PositionedGlyph],
    font_bytes: &[u8],
    units_per_em: u16,
    font_size: f32,
    color: Rgba,
) {
    if color.a == 0 || units_per_em == 0 {
        return;
    }
    let Some(face) = glyph::parse_face(font_bytes) else {
        return;
    };
    let scale = font_size / units_per_em as f32;
    let mut sink = PathSink::default();
    for g in glyphs {
        let pen = Pen {
            origin_x: f.x + g.x,
            baseline_y: f.y + g.y,
            scale,
        };
        glyph::trace_glyph(&face, g.glyph_id, pen, &mut sink);
    }
    if !sink.0.is_empty() {
        let _ = writeln!(
            svg,
            "<path d=\"{}\" {}/>",
            sink.0.trim_end(),
            fill_attrs(color)
        );
    }
}
