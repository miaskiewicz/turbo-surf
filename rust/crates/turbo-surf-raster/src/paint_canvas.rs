//! Default (tiny-skia) backend for the 2D-canvas op-log painter.
//!
//! Replays the shared [`crate::canvas_ops`] op stream into a tiny-skia pixmap and
//! encodes PNG. The pixmap starts fully transparent (the canvas initial state), so
//! an empty op-log yields a valid blank image; fills composite source-over, and
//! `clearRect` punches back to transparent. The opt-in wgpu→Metal backend
//! ([`crate::paint_canvas_gpu`]) paints the same op stream on the real GPU.

use tiny_skia::{BlendMode, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
use turbo_html2pdf_core::Rgba;

use crate::canvas_ops::{self, CanvasBackend, Op, SubPath};

/// Rasterize the parsed op stream at `width × height` (both already clamped ≥ 1 by
/// the caller) into a PNG. `Err` only on pixmap allocation or PNG encoding.
pub(crate) fn replay(width: u32, height: u32, ops: &[Op]) -> Result<Vec<u8>, String> {
    let pm = Pixmap::new(width, height).ok_or_else(|| format!("bad canvas {width}x{height}"))?;
    let mut backend = SkiaCanvas { pm };
    canvas_ops::run(ops, &mut backend);
    backend
        .pm
        .encode_png()
        .map_err(|e| format!("png encode: {e}"))
}

/// A tiny-skia pixmap wearing the [`CanvasBackend`] primitives.
struct SkiaCanvas {
    pm: Pixmap,
}

impl SkiaCanvas {
    /// Build a tiny-skia path from device-space subpaths (each `move_to` + `line_to`
    /// run, closed when the subpath is). `None` if nothing traceable remains.
    fn build_path(subs: &[SubPath]) -> Option<tiny_skia::Path> {
        let mut b = PathBuilder::new();
        for sub in subs {
            let mut pts = sub.pts.iter();
            if let Some(&(x, y)) = pts.next() {
                b.move_to(x, y);
                for &(x, y) in pts {
                    b.line_to(x, y);
                }
                if sub.closed {
                    b.close();
                }
            }
        }
        b.finish()
    }
}

/// A solid, anti-aliased tiny-skia paint.
fn solid(c: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(c.r, c.g, c.b, c.a);
    paint.anti_alias = true;
    paint
}

impl CanvasBackend for SkiaCanvas {
    fn fill(&mut self, subs: &[SubPath], color: Rgba) {
        if color.a == 0 {
            return;
        }
        if let Some(path) = Self::build_path(subs) {
            self.pm.fill_path(
                &path,
                &solid(color),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    fn stroke(&mut self, subs: &[SubPath], color: Rgba, width: f32) {
        if color.a == 0 || width <= 0.0 {
            return;
        }
        if let Some(path) = Self::build_path(subs) {
            let stroke = Stroke {
                width,
                ..Default::default()
            };
            self.pm
                .stroke_path(&path, &solid(color), &stroke, Transform::identity(), None);
        }
    }

    fn clear(&mut self, quad: &[(f32, f32); 4]) {
        // `Clear` blend writes transparent black regardless of source colour.
        let mut b = PathBuilder::new();
        b.move_to(quad[0].0, quad[0].1);
        for &(x, y) in &quad[1..] {
            b.line_to(x, y);
        }
        b.close();
        if let Some(path) = b.finish() {
            let paint = Paint {
                blend_mode: BlendMode::Clear,
                ..Default::default()
            };
            self.pm.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_dims(bytes: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(bytes).expect("decode png");
        (img.width(), img.height())
    }

    #[test]
    fn empty_ops_yield_blank_png_at_size() {
        let png = replay(40, 30, &[]).unwrap();
        assert_eq!(png_dims(&png), (40, 30));
    }

    // The op-log is the render tier's `ctx._ops` tuple format:
    // [name, argsArray, fillStyle, strokeStyle, font, textBaseline, textAlign, globalAlpha, comp].
    #[test]
    fn fill_rect_paints_its_colour() {
        let ops = canvas_ops::parse_ops(
            r##"[["fillRect",[0,0,16,16],"#ff0000","#000000","10px sans-serif","alphabetic","start",1,"source-over"]]"##,
        )
        .unwrap();
        let png = replay(16, 16, &ops).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        let px = img.get_pixel(8, 8);
        assert_eq!(px.0, [255, 0, 0, 255], "centre pixel is opaque red");
    }

    #[test]
    fn distinct_op_lists_differ() {
        let a = replay(
            20, 20,
            &canvas_ops::parse_ops(r##"[["fillRect",[0,0,20,20],"#000000","#000","10px x","alphabetic","start",1,"source-over"]]"##).unwrap(),
        ).unwrap();
        let b = replay(
            20, 20,
            &canvas_ops::parse_ops(r##"[["fillRect",[0,0,5,5],"#000000","#000","10px x","alphabetic","start",1,"source-over"]]"##).unwrap(),
        ).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn fill_text_renders_non_blank() {
        let ops = canvas_ops::parse_ops(
            r##"[["fillText",["Ag",2,22],"#000000","#000","20px sans-serif","alphabetic","start",1,"source-over"]]"##,
        )
        .unwrap();
        let png = replay(64, 32, &ops).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        let inked = img.pixels().filter(|p| p.0[3] > 0).count();
        assert!(inked > 0, "text painted some opaque pixels");
    }

    #[test]
    fn clear_rect_punches_transparent() {
        let ops = canvas_ops::parse_ops(
            r##"[["fillRect",[0,0,16,16],"#0000ff","#000","10px x","alphabetic","start",1,"source-over"],["clearRect",[4,4,8,8],"#0000ff","#000","10px x","alphabetic","start",1,"source-over"]]"##,
        )
        .unwrap();
        let png = replay(16, 16, &ops).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(8, 8).0[3], 0, "cleared centre is transparent");
        assert_eq!(img.get_pixel(1, 1).0, [0, 0, 255, 255], "corner still blue");
    }
}
