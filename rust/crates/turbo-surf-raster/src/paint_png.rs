//! Raster painter: walk a [`Fragment`] galley into a tiny-skia pixmap and encode
//! PNG. Children paint back-to-front in CSS stacking order (§9.9); decoded images
//! are scaled into their box, others fall back to a placeholder. Content past the
//! pixmap edge is clipped by tiny-skia, giving a viewport-clipped screenshot.

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};
use turbo_html2pdf_core::layout::value::{BorderEdges, BorderSide};
use turbo_html2pdf_core::{Fragment, FragmentContent, PositionedGlyph, Rgba};

use crate::glyph::{self, Pen, Tracer};
use crate::image_paint::DecodedAssets;

/// Neutral fill for an `<img>` box whose pixels we don't have.
const IMAGE_PLACEHOLDER: Rgba = Rgba {
    r: 220,
    g: 220,
    b: 220,
    a: 255,
};

/// Paint `galley` into a `width × height` PNG over a `bg` canvas fill (the
/// propagated root/body background). `Err` only on a zero/oversized canvas or a
/// PNG-encode failure.
pub fn paint(
    galley: &Fragment,
    width: u32,
    height: u32,
    bg: Rgba,
    images: &DecodedAssets,
) -> Result<Vec<u8>, String> {
    let mut pm =
        Pixmap::new(width, height).ok_or_else(|| format!("bad canvas {width}x{height}"))?;
    pm.fill(tiny_skia::Color::from_rgba8(bg.r, bg.g, bg.b, bg.a));
    paint_fragment(&mut pm, galley, images);
    pm.encode_png().map_err(|e| format!("png encode: {e}"))
}

fn paint_fragment(pm: &mut Pixmap, f: &Fragment, images: &DecodedAssets) {
    match &f.content {
        FragmentContent::Box { background, border } => {
            if let Some(bg) = background {
                fill_rect(pm, f.x, f.y, f.width, f.height, *bg);
            }
            paint_border(pm, f, border);
        }
        FragmentContent::TextLine {
            glyphs,
            face,
            font_size,
            color,
        } => paint_text(
            pm,
            f,
            glyphs,
            face.data(),
            face.units_per_em(),
            *font_size,
            *color,
        ),
        FragmentContent::Image(placement) => paint_image(pm, f, &placement.name, images),
        FragmentContent::Directive(_) => {}
    }
    // Paint children back-to-front in CSS stacking order (§9.9), not raw DOM
    // order, so `position`/`z-index` boxes (menus, modals) layer correctly.
    for &i in &f.paint_order() {
        paint_fragment(pm, &f.children[i], images);
    }
}

/// Draw an image into its layout box: scale the decoded pixels and blit them;
/// fall back to a neutral placeholder when the image is absent or won't scale.
fn paint_image(pm: &mut Pixmap, f: &Fragment, name: &str, images: &DecodedAssets) {
    let (w, h) = (f.width.round() as u32, f.height.round() as u32);
    let img = images.get(name).and_then(|d| d.scaled_pixmap(w, h));
    match img {
        Some(src) => {
            pm.draw_pixmap(
                f.x.round() as i32,
                f.y.round() as i32,
                src.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        None => fill_rect(pm, f.x, f.y, f.width, f.height, IMAGE_PLACEHOLDER),
    }
}

fn solid(c: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(c.r, c.g, c.b, c.a);
    paint.anti_alias = true;
    paint
}

fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
    if w <= 0.0 || h <= 0.0 || c.a == 0 {
        return;
    }
    if let Some(rect) = Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(rect, &solid(c), Transform::identity(), None);
    }
}

/// Paint each border side as a filled edge band (approximation: no corner
/// mitring, no line styles — width + color only).
fn paint_border(pm: &mut Pixmap, f: &Fragment, b: &BorderEdges) {
    let side = |s: &BorderSide| s.color.filter(|_| s.width > 0).map(|c| (s.width as f32, c));
    if let Some((w, c)) = side(&b.top) {
        fill_rect(pm, f.x, f.y, f.width, w, c);
    }
    if let Some((w, c)) = side(&b.bottom) {
        fill_rect(pm, f.x, f.y + f.height - w, f.width, w, c);
    }
    if let Some((w, c)) = side(&b.left) {
        fill_rect(pm, f.x, f.y, w, f.height, c);
    }
    if let Some((w, c)) = side(&b.right) {
        fill_rect(pm, f.x + f.width - w, f.y, w, f.height, c);
    }
}

/// A tiny-skia path sink for [`glyph::trace_glyph`].
struct PathSink(PathBuilder);

impl Tracer for PathSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.0.quad_to(cx, cy, x, y);
    }
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.0.cubic_to(c1x, c1y, c2x, c2y, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

fn paint_text(
    pm: &mut Pixmap,
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
    let paint = solid(color);
    for g in glyphs {
        let pen = Pen {
            origin_x: f.x + g.x,
            baseline_y: f.y + g.y,
            scale,
        };
        let mut sink = PathSink(PathBuilder::new());
        glyph::trace_glyph(&face, g.glyph_id, pen, &mut sink);
        if let Some(path) = sink.0.finish() {
            pm.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}
