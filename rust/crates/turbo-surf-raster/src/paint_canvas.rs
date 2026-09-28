//! Canvas replay: turn a recorded 2D-canvas draw list into a real rasterized PNG.
//!
//! A page's own `<canvas>` painting happens in JS via `CanvasRenderingContext2D`.
//! The render tier can record every draw call the page made (the op name, its
//! numeric/string args, and the drawing state — `fillStyle`/`strokeStyle`/`font`/
//! `globalAlpha`/… — snapshotted at the call) into a JSON list; this module replays
//! that list into a tiny-skia pixmap so the resulting PNG is *content-dependent*
//! (real rects, real glyph outlines), not a fixed-size stub.
//!
//! It is a deliberately minimal replay: `fillRect`/`strokeRect`/`clearRect`,
//! `fillText`/`strokeText`, and path build (`moveTo`/`lineTo`/`rect`/`arc`/
//! `closePath`) + `fill`/`stroke`, all under a `save`/`restore` transform stack
//! (`translate`/`scale`/`rotate`/`transform`/`setTransform`). Pixel sources
//! (`drawImage`/`putImageData`) are unavailable to a browserless engine, so they
//! paint a neutral placeholder rather than error. Unknown ops are skipped.

use tiny_skia::{BlendMode, FillRule, Paint, Path, PathBuilder, Pixmap, Point, Stroke, Transform};
use turbo_html2pdf_core::text::{FontFace, FontRegistry};
use turbo_html2pdf_core::Rgba;

use crate::glyph::{self, Pen, Tracer};

/// Neutral fill for a `drawImage`/`putImageData` box whose pixels we don't have
/// (a browserless engine has no bitmap source), matching the layout painter's
/// missing-image placeholder.
const IMAGE_PLACEHOLDER: Rgba = Rgba {
    r: 220,
    g: 220,
    b: 220,
    a: 255,
};

/// Default canvas line width — the tuple carries no `lineWidth`, so strokes use the
/// CSS canvas default of 1 device px.
const DEFAULT_LINE_WIDTH: f32 = 1.0;

/// Replay a recorded 2D-canvas draw list (`ops_json`) into a `width × height` PNG.
///
/// `ops_json` is a JSON array; each element is a fixed tuple
/// `[name, argsArray, fillStyle, strokeStyle, font, textBaseline, textAlign,
/// globalAlpha, globalCompositeOperation]` — the op and the drawing state captured
/// at the call. The pixmap starts transparent; an empty list still yields a valid
/// PNG of the requested size. `Err` only on a JSON parse failure or a PNG encode
/// failure — never on an unknown op or a missing pixel source.
pub fn canvas_ops_png(width: u32, height: u32, ops_json: &str) -> Result<Vec<u8>, String> {
    let ops = Json::parse(ops_json).map_err(|e| format!("canvas ops parse: {e}"))?;
    let mut pm = Pixmap::new(width.max(1), height.max(1))
        .ok_or_else(|| format!("bad canvas {width}x{height}"))?;
    // Bundled fallback faces only — canvas text must render deterministically and
    // offline, so we don't reach for the machine's system fonts here.
    let reg = crate::font_registry(false);
    let mut canvas = Canvas::new(&mut pm, &reg);
    if let Json::Arr(list) = ops {
        for op in &list {
            canvas.replay(op);
        }
    }
    pm.encode_png().map_err(|e| format!("png encode: {e}"))
}

/// One segment of the current path, already mapped into device space (the current
/// transform is applied when the point is added, matching canvas semantics where a
/// path point is fixed by the CTM in effect at the time it is recorded).
#[derive(Clone, Copy)]
enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Close,
}

/// Replay state: the target pixmap, the current transform matrix (CTM) + its
/// save/restore stack, the font registry, and the path being built.
struct Canvas<'a> {
    pm: &'a mut Pixmap,
    reg: &'a FontRegistry,
    ctm: Transform,
    stack: Vec<Transform>,
    path: Vec<Seg>,
}

impl<'a> Canvas<'a> {
    fn new(pm: &'a mut Pixmap, reg: &'a FontRegistry) -> Self {
        Canvas {
            pm,
            reg,
            ctm: Transform::identity(),
            stack: Vec::new(),
            path: Vec::new(),
        }
    }

    /// Map a point from user space through the current transform into device space.
    fn dev(&self, x: f32, y: f32) -> (f32, f32) {
        let mut p = Point::from_xy(x, y);
        self.ctm.map_point(&mut p);
        (p.x, p.y)
    }

    /// Dispatch one recorded op tuple. State (`fillStyle` etc.) is read per op from
    /// the tuple, so only the CTM (via `save`/`restore`/transform ops) and the
    /// current path carry across ops.
    fn replay(&mut self, op: &Json) {
        let Json::Arr(t) = op else { return };
        let name = match t.first().and_then(Json::as_str) {
            Some(n) => n,
            None => return,
        };
        let args = match t.get(1) {
            Some(Json::Arr(a)) => a.as_slice(),
            _ => &[],
        };
        let state = OpState::from_tuple(t);
        match name {
            // --- rectangles ---
            "fillRect" => self.rect_op(args, state.fill(), false, false),
            "strokeRect" => self.rect_op(args, state.stroke(), true, false),
            "clearRect" => self.rect_op(
                args,
                Rgba {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                },
                false,
                true,
            ),
            // --- text ---
            "fillText" => self.text_op(args, &state, false),
            "strokeText" => self.text_op(args, &state, true),
            // --- path building ---
            "beginPath" => self.path.clear(),
            "moveTo" => self.move_to(args),
            "lineTo" => self.line_to(args),
            "rect" => self.rect_path(args),
            "arc" => self.arc(args),
            "closePath" => self.path.push(Seg::Close),
            "fill" => self.fill_path(state.fill()),
            "stroke" => self.stroke_path(state.stroke()),
            // --- transform stack ---
            "save" => self.stack.push(self.ctm),
            "restore" => {
                if let Some(t) = self.stack.pop() {
                    self.ctm = t;
                }
            }
            "translate" => self.pre(Transform::from_translate(num(args, 0), num(args, 1))),
            "scale" => self.pre(Transform::from_scale(num(args, 0), num(args, 1))),
            "rotate" => self.pre(Transform::from_rotate(num(args, 0).to_degrees())),
            "transform" => self.pre(matrix(args)),
            "setTransform" => self.ctm = matrix(args),
            // --- pixel sources we can't honor browserlessly ---
            "drawImage" | "putImageData" => self.placeholder(args),
            // Everything else (stateful setters, hints, ellipse, …) is a no-op.
            _ => {}
        }
    }

    /// Pre-concat `m` onto the current transform (canvas transform ops post-multiply
    /// the CTM, i.e. they compose in the box's local space).
    fn pre(&mut self, m: Transform) {
        self.ctm = self.ctm.pre_concat(m);
    }

    /// `fillRect`/`strokeRect`/`clearRect`: draw an independent rectangle (its four
    /// device-space corners) — not the current path — in the given colour.
    fn rect_op(&mut self, args: &[Json], color: Rgba, stroke: bool, clear: bool) {
        let (x, y, w, h) = (num(args, 0), num(args, 1), num(args, 2), num(args, 3));
        if w == 0.0 || h == 0.0 {
            return;
        }
        let Some(path) = self.rect_corners(x, y, w, h) else {
            return;
        };
        if clear {
            let paint = Paint {
                blend_mode: BlendMode::Clear,
                ..Paint::default()
            };
            self.pm.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        } else if stroke {
            self.stroke(&path, color);
        } else {
            self.fill(&path, color);
        }
    }

    /// A closed rectangle path from four corners mapped through the current CTM (so a
    /// rotated/scaled context yields a rotated/scaled quad, not an axis-aligned box).
    fn rect_corners(&self, x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
        let mut pb = PathBuilder::new();
        let (x0, y0) = self.dev(x, y);
        let (x1, y1) = self.dev(x + w, y);
        let (x2, y2) = self.dev(x + w, y + h);
        let (x3, y3) = self.dev(x, y + h);
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        pb.line_to(x2, y2);
        pb.line_to(x3, y3);
        pb.close();
        pb.finish()
    }

    fn move_to(&mut self, args: &[Json]) {
        let (x, y) = self.dev(num(args, 0), num(args, 1));
        self.path.push(Seg::Move(x, y));
    }

    fn line_to(&mut self, args: &[Json]) {
        let (x, y) = self.dev(num(args, 0), num(args, 1));
        self.path.push(Seg::Line(x, y));
    }

    /// `rect(x,y,w,h)` adds a new closed rectangle subpath to the current path.
    fn rect_path(&mut self, args: &[Json]) {
        let (x, y, w, h) = (num(args, 0), num(args, 1), num(args, 2), num(args, 3));
        for (i, (px, py)) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
            .into_iter()
            .enumerate()
        {
            let (dx, dy) = self.dev(px, py);
            self.path.push(if i == 0 {
                Seg::Move(dx, dy)
            } else {
                Seg::Line(dx, dy)
            });
        }
        self.path.push(Seg::Close);
    }

    /// `arc(cx,cy,r,start,end[,ccw])` tessellated into line segments (a minimally
    /// correct arc: enough facets to read as a curve, connected from the current
    /// point when one exists).
    fn arc(&mut self, args: &[Json]) {
        let (cx, cy, r) = (num(args, 0), num(args, 1), num(args, 2));
        let (a0, a1) = (num(args, 3), num(args, 4));
        let ccw = bool_arg(args, 5);
        if r <= 0.0 {
            return;
        }
        let mut sweep = a1 - a0;
        // Normalize the swept angle to the drawing direction, as the canvas does.
        if ccw && sweep > 0.0 {
            sweep -= std::f32::consts::TAU;
        } else if !ccw && sweep < 0.0 {
            sweep += std::f32::consts::TAU;
        }
        let steps = ((sweep.abs() / (std::f32::consts::PI / 16.0)).ceil() as usize).max(2);
        let start_new = self.path.is_empty();
        for i in 0..=steps {
            let a = a0 + sweep * (i as f32 / steps as f32);
            let (dx, dy) = self.dev(cx + r * a.cos(), cy + r * a.sin());
            self.path.push(if i == 0 && start_new {
                Seg::Move(dx, dy)
            } else {
                Seg::Line(dx, dy)
            });
        }
    }

    /// Fill the current path with the current fill colour (device-space, so the CTM
    /// is already baked into the points).
    fn fill_path(&mut self, color: Rgba) {
        if let Some(path) = build_path(&self.path) {
            self.fill(&path, color);
        }
    }

    /// Stroke the current path with the current stroke colour.
    fn stroke_path(&mut self, color: Rgba) {
        if let Some(path) = build_path(&self.path) {
            self.stroke(&path, color);
        }
    }

    fn fill(&mut self, path: &Path, color: Rgba) {
        if color.a == 0 {
            return;
        }
        self.pm.fill_path(
            path,
            &solid(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    fn stroke(&mut self, path: &Path, color: Rgba) {
        if color.a == 0 {
            return;
        }
        let stroke = Stroke {
            width: DEFAULT_LINE_WIDTH,
            ..Stroke::default()
        };
        self.pm
            .stroke_path(path, &solid(color), &stroke, Transform::identity(), None);
    }

    /// `fillText`/`strokeText`: shape the string against the selected face and trace
    /// each glyph outline, placing the run at `(x, y)` per `textAlign`/`textBaseline`.
    fn text_op(&mut self, args: &[Json], state: &OpState, stroke: bool) {
        let Some(text) = args.first().and_then(Json::as_str) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let (x, y) = (num(args, 1), num(args, 2));
        let (size, family, weight, italic) = parse_font(state.font);
        let Some(face) = select_face(self.reg, &family, weight, italic) else {
            return;
        };
        let color = if stroke { state.stroke() } else { state.fill() };
        if color.a == 0 {
            return;
        }
        // Horizontal alignment shifts the run origin by its advance width.
        let width = face.measure(text, size, 0.0);
        let ox = x + align_shift(state.align, width);
        // Vertical baseline placement relative to the requested `textBaseline`.
        let baseline = y + baseline_shift(state.baseline, face, size);

        let scale = size / f32::from(face.units_per_em().max(1));
        let Some(ttf) = glyph::parse_face(face.data(), face.index()) else {
            return;
        };
        let paint = solid(color);
        let mut pen_x = ox;
        for g in face.shape(text) {
            let pen = Pen {
                origin_x: pen_x + g.x_offset as f32 * scale,
                baseline_y: baseline - g.y_offset as f32 * scale,
                scale,
            };
            let mut sink = PathSink(PathBuilder::new());
            glyph::trace_glyph(&ttf, g.glyph_id, pen, &mut sink);
            if let Some(path) = sink.0.finish() {
                if stroke {
                    let s = Stroke {
                        width: DEFAULT_LINE_WIDTH,
                        ..Stroke::default()
                    };
                    self.pm
                        .stroke_path(&path, &paint, &s, Transform::identity(), None);
                } else {
                    self.pm.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            pen_x += g.x_advance as f32 * scale;
        }
    }

    /// `drawImage`/`putImageData`: the source bitmap is unavailable browserlessly, so
    /// paint a neutral placeholder over the destination box when we can size it (the
    /// trailing four numeric args are the `dx,dy,dw,dh` for every arg-count form).
    fn placeholder(&mut self, args: &[Json]) {
        let nums: Vec<f32> = args.iter().filter_map(Json::as_f32).collect();
        if nums.len() < 4 {
            return; // no destination rectangle we can place
        }
        let (dx, dy, dw, dh) = {
            let n = nums.len();
            (nums[n - 4], nums[n - 3], nums[n - 2], nums[n - 1])
        };
        if dw == 0.0 || dh == 0.0 {
            return;
        }
        if let Some(path) = self.rect_corners(dx, dy, dw, dh) {
            self.fill(&path, IMAGE_PLACEHOLDER);
        }
    }
}

/// The drawing state captured alongside one op (tuple indices 2..=8).
struct OpState<'a> {
    fill: &'a str,
    stroke: &'a str,
    font: &'a str,
    baseline: &'a str,
    align: &'a str,
    alpha: f32,
}

impl<'a> OpState<'a> {
    fn from_tuple(t: &'a [Json]) -> Self {
        let s = |i: usize| t.get(i).and_then(Json::as_str).unwrap_or("");
        OpState {
            fill: s(2),
            stroke: s(3),
            font: s(4),
            baseline: s(5),
            align: s(6),
            alpha: t
                .get(7)
                .and_then(Json::as_f32)
                .unwrap_or(1.0)
                .clamp(0.0, 1.0),
        }
    }

    /// The effective fill colour: `fillStyle` (default black) scaled by `globalAlpha`.
    fn fill(&self) -> Rgba {
        apply_alpha(parse_color(self.fill).unwrap_or(BLACK), self.alpha)
    }

    /// The effective stroke colour: `strokeStyle` (default black) scaled by alpha.
    fn stroke(&self) -> Rgba {
        apply_alpha(parse_color(self.stroke).unwrap_or(BLACK), self.alpha)
    }
}

const BLACK: Rgba = Rgba {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};

/// Scale a colour's alpha by `globalAlpha`.
fn apply_alpha(c: Rgba, alpha: f32) -> Rgba {
    Rgba {
        a: (c.a as f32 * alpha).round().clamp(0.0, 255.0) as u8,
        ..c
    }
}

fn solid(c: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(c.r, c.g, c.b, c.a);
    paint.anti_alias = true;
    paint
}

/// Build a tiny-skia path from device-space segments; `None` if it has no drawable
/// geometry.
fn build_path(segs: &[Seg]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for s in segs {
        match *s {
            Seg::Move(x, y) => pb.move_to(x, y),
            // A `line_to`/`close` before any `move_to` is a no-op in the builder.
            Seg::Line(x, y) => pb.line_to(x, y),
            Seg::Close => pb.close(),
        }
    }
    pb.finish()
}

/// The nth arg as an f32 (0.0 if absent or non-numeric — object args were
/// pre-serialized to tag strings, so a non-number where a number is expected is
/// simply ignored).
fn num(args: &[Json], i: usize) -> f32 {
    args.get(i).and_then(Json::as_f32).unwrap_or(0.0)
}

/// The nth arg as a boolean (canvas `arc`'s `counterclockwise` flag).
fn bool_arg(args: &[Json], i: usize) -> bool {
    matches!(args.get(i), Some(Json::Bool(true)))
}

/// A 6-value affine matrix from `transform`/`setTransform` args `[a,b,c,d,e,f]`,
/// falling back to identity for missing components.
fn matrix(args: &[Json]) -> Transform {
    Transform::from_row(
        num_or(args, 0, 1.0),
        num_or(args, 1, 0.0),
        num_or(args, 2, 0.0),
        num_or(args, 3, 1.0),
        num_or(args, 4, 0.0),
        num_or(args, 5, 0.0),
    )
}

fn num_or(args: &[Json], i: usize, default: f32) -> f32 {
    args.get(i).and_then(Json::as_f32).unwrap_or(default)
}

/// Horizontal origin shift for `textAlign` over a run of `width` px.
fn align_shift(align: &str, width: f32) -> f32 {
    match align {
        "center" => -width / 2.0,
        "right" | "end" => -width,
        _ => 0.0, // start / left / unset
    }
}

/// Vertical baseline shift for `textBaseline` (the arg `y` is the baseline for the
/// default `alphabetic`).
fn baseline_shift(baseline: &str, face: &FontFace, size: f32) -> f32 {
    match baseline {
        "top" | "hanging" => face.ascent_px(size),
        "middle" => (face.ascent_px(size) - face.descent_px(size)) / 2.0,
        "bottom" | "ideographic" => -face.descent_px(size),
        _ => 0.0, // alphabetic
    }
}

/// Parse a CSS font shorthand (e.g. `"14px 'Arial'"`, `"bold 16px sans-serif"`)
/// into `(size_px, family_css, weight, italic)`. Only the pixel size + family are
/// recovered precisely; weight/style are the coarse bold/italic flags the face
/// selector needs.
fn parse_font(css: &str) -> (f32, String, u16, bool) {
    let mut size = 10.0_f32; // CSS canvas default (10px sans-serif)
    let mut weight = 400_u16;
    let mut italic = false;
    let mut family = String::from("sans-serif");
    let tokens: Vec<&str> = css.split_whitespace().collect();
    // The size token ends in a length unit (…px / …pt); everything after it is the
    // family list, everything before it holds style/weight/variant keywords.
    if let Some(idx) = tokens
        .iter()
        .position(|t| t.ends_with("px") || t.ends_with("pt"))
    {
        let tok = tokens[idx];
        let digits: String = tok
            .trim_end_matches(|c: char| c.is_ascii_alphabetic())
            .to_string();
        if let Ok(v) = digits.parse::<f32>() {
            size = if tok.ends_with("pt") {
                v * 96.0 / 72.0
            } else {
                v
            };
        }
        for t in &tokens[..idx] {
            match *t {
                "bold" | "bolder" => weight = 700,
                "italic" | "oblique" => italic = true,
                _ => {
                    if let Ok(w) = t.parse::<u16>() {
                        weight = w;
                    }
                }
            }
        }
        let rest = tokens[idx + 1..].join(" ");
        if !rest.trim().is_empty() {
            family = rest;
        }
    }
    (size.max(1.0), family, weight, italic)
}

/// Select a face for a CSS `font-family` list (comma-separated, quotes trimmed),
/// mirroring [`crate::measure_text`]'s selection.
fn select_face<'r>(
    reg: &'r FontRegistry,
    family_css: &str,
    weight: u16,
    italic: bool,
) -> Option<&'r FontFace> {
    let families: Vec<String> = family_css
        .split(',')
        .map(|s| {
            s.trim()
                .trim_matches(|c| c == '\'' || c == '"')
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    let mut refs: Vec<&str> = families.iter().map(String::as_str).collect();
    if refs.is_empty() {
        refs.push("sans-serif");
    }
    reg.select(&refs, weight, italic)
}

/// Parse a CSS colour: `#rgb`, `#rrggbb`, `rgb(...)`, `rgba(...)`, `transparent`,
/// and a small named subset. `None` (→ the caller's default) for anything else.
fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(inner) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")) {
        return parse_rgb(inner.trim_end_matches(')'));
    }
    named_color(&s.to_ascii_lowercase())
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    let bytes = hex.as_bytes();
    let h = |c: u8| (c as char).to_digit(16);
    match bytes.len() {
        3 => {
            let (r, g, b) = (h(bytes[0])?, h(bytes[1])?, h(bytes[2])?);
            Some(Rgba {
                r: (r * 17) as u8,
                g: (g * 17) as u8,
                b: (b * 17) as u8,
                a: 255,
            })
        }
        6 => Some(Rgba {
            r: u8::from_str_radix(&hex[0..2], 16).ok()?,
            g: u8::from_str_radix(&hex[2..4], 16).ok()?,
            b: u8::from_str_radix(&hex[4..6], 16).ok()?,
            a: 255,
        }),
        _ => None,
    }
}

fn parse_rgb(inner: &str) -> Option<Rgba> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() < 3 {
        return None;
    }
    let ch = |p: &str| {
        p.parse::<f32>()
            .ok()
            .map(|v| v.round().clamp(0.0, 255.0) as u8)
    };
    Some(Rgba {
        r: ch(parts[0])?,
        g: ch(parts[1])?,
        b: ch(parts[2])?,
        a: parts
            .get(3)
            .and_then(|p| p.parse::<f32>().ok())
            .map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
            .unwrap_or(255),
    })
}

/// A small subset of the CSS named colours (the ones canvas demos actually use).
fn named_color(name: &str) -> Option<Rgba> {
    let rgb = |r, g, b| Some(Rgba { r, g, b, a: 255 });
    match name {
        "transparent" => Some(Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }),
        "black" => rgb(0, 0, 0),
        "white" => rgb(255, 255, 255),
        "red" => rgb(255, 0, 0),
        "green" => rgb(0, 128, 0),
        "lime" => rgb(0, 255, 0),
        "blue" => rgb(0, 0, 255),
        "yellow" => rgb(255, 255, 0),
        "cyan" | "aqua" => rgb(0, 255, 255),
        "magenta" | "fuchsia" => rgb(255, 0, 255),
        "orange" => rgb(255, 165, 0),
        "purple" => rgb(128, 0, 128),
        "gray" | "grey" => rgb(128, 128, 128),
        "silver" => rgb(192, 192, 192),
        "maroon" => rgb(128, 0, 0),
        "navy" => rgb(0, 0, 128),
        "teal" => rgb(0, 128, 128),
        "olive" => rgb(128, 128, 0),
        _ => None,
    }
}

/// A tiny-skia path sink for [`glyph::trace_glyph`] (mirrors the layout painter's).
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

// --- minimal JSON parser -----------------------------------------------------
//
// The crate carries no serde dependency, and the op list is a small, well-shaped
// array of arrays, so a compact recursive-descent parser is enough. Objects (which
// the recorder pre-serializes to tag strings before they reach us) parse but are
// discarded.

/// A parsed JSON value (objects collapse to [`Json::Null`] — see the module note).
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
}

impl Json {
    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_f32(&self) -> Option<f32> {
        match self {
            Json::Num(n) => Some(*n as f32),
            _ => None,
        }
    }

    fn parse(src: &str) -> Result<Json, String> {
        let mut p = Parser {
            bytes: src.as_bytes(),
            pos: 0,
        };
        p.ws();
        let v = p.value()?;
        p.ws();
        Ok(v)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.peek() {
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected byte {:?}", c as char)),
            None => Err("unexpected end of input".into()),
        }
    }

    fn literal(&mut self, lit: &str, val: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(lit.as_bytes()) {
            self.pos += lit.len();
            Ok(val)
        } else {
            Err(format!("expected `{lit}`"))
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err("expected `,` or `]`".into()),
            }
        }
    }

    /// Parse (and discard the contents of) an object — the recorder never emits raw
    /// objects into our tuples, but we accept them so a stray one doesn't fail.
    fn object(&mut self) -> Result<Json, String> {
        self.pos += 1; // '{'
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Null);
        }
        loop {
            self.ws();
            let _key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err("expected `:`".into());
            }
            self.pos += 1;
            let _ = self.value()?;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Null);
                }
                _ => return Err("expected `,` or `}`".into()),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.peek() != Some(b'"') {
            return Err("expected string".into());
        }
        self.pos += 1;
        let mut out = String::new();
        while let Some(b) = self.peek() {
            self.pos += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.peek().ok_or("unterminated escape")?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'u' => out.push(self.unicode_escape()?),
                        other => out.push(other as char),
                    }
                }
                // Continuation bytes of a multi-byte UTF-8 sequence: rebuild the char.
                _ if b < 0x80 => out.push(b as char),
                _ => {
                    let start = self.pos - 1;
                    while self
                        .peek()
                        .map(|c| (0x80..0xC0).contains(&c))
                        .unwrap_or(false)
                    {
                        self.pos += 1;
                    }
                    if let Ok(s) = std::str::from_utf8(&self.bytes[start..self.pos]) {
                        out.push_str(s);
                    }
                }
            }
        }
        Err("unterminated string".into())
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let hex = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or("short \\u escape")?;
        let code = u32::from_str_radix(std::str::from_utf8(hex).map_err(|_| "bad \\u escape")?, 16)
            .map_err(|_| "bad \\u escape")?;
        self.pos += 4;
        Ok(char::from_u32(code).unwrap_or('\u{FFFD}'))
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| "bad number")?;
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("bad number `{text}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a PNG's IHDR width/height (bytes 16..24, big-endian) after checking the
    /// 8-byte PNG signature — a dependency-free validity + size check.
    fn png_dims(bytes: &[u8]) -> (u32, u32) {
        const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(bytes.len() > 24, "png too short");
        assert_eq!(&bytes[..8], &SIG, "bad png signature");
        assert_eq!(&bytes[12..16], b"IHDR", "first chunk not IHDR");
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        (w, h)
    }

    #[test]
    fn empty_ops_yield_valid_png_of_size() {
        let png = canvas_ops_png(120, 80, "[]").expect("empty ops render");
        assert_eq!(png_dims(&png), (120, 80));
    }

    #[test]
    fn zero_size_clamps_to_one() {
        let png = canvas_ops_png(0, 0, "[]").expect("zero-size render");
        assert_eq!(png_dims(&png), (1, 1));
    }

    #[test]
    fn different_content_yields_different_bytes() {
        // Two draw lists that differ only in the painted content must not encode to
        // the same PNG — proving the raster is content-dependent, not a stub.
        let rect_a = r##"[["fillRect",[10,10,40,40],"#ff0000","#000","10px sans-serif","alphabetic","start",1,"source-over"]]"##;
        let rect_b = r##"[["fillRect",[10,10,40,40],"#0000ff","#000","10px sans-serif","alphabetic","start",1,"source-over"]]"##;
        let a = canvas_ops_png(80, 80, rect_a).expect("a");
        let b = canvas_ops_png(80, 80, rect_b).expect("b");
        assert_ne!(a, b, "different fill colours must differ");

        let text_a = r##"[["fillText",["Hello",10,40],"#000","#000","20px sans-serif","alphabetic","start",1,"source-over"]]"##;
        let text_b = r##"[["fillText",["World",10,40],"#000","#000","20px sans-serif","alphabetic","start",1,"source-over"]]"##;
        let ta = canvas_ops_png(160, 60, text_a).expect("ta");
        let tb = canvas_ops_png(160, 60, text_b).expect("tb");
        assert_ne!(ta, tb, "different text must differ");
    }

    #[test]
    fn simple_content_is_well_over_200_bytes() {
        let ops = r##"[
            ["fillRect",[5,5,90,40],"#3366cc","#000","16px sans-serif","alphabetic","start",1,"source-over"],
            ["fillText",["turbo-surf",10,80],"#ffffff","#000","16px 'Arial'","alphabetic","start",1,"source-over"]
        ]"##;
        let png = canvas_ops_png(200, 120, ops).expect("render");
        assert_eq!(png_dims(&png), (200, 120));
        assert!(
            png.len() > 200,
            "expected a plausibly-sized PNG, got {} bytes",
            png.len()
        );
    }

    #[test]
    fn path_fill_and_transform_replay() {
        // Exercise the path + transform stack: a translated, filled triangle. Must
        // differ from an empty canvas and stay a valid PNG.
        let ops = r##"[
            ["save",[],"#000","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["translate",[20,20],"#000","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["beginPath",[],"#0a0","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["moveTo",[0,0],"#0a0","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["lineTo",[40,0],"#0a0","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["lineTo",[20,40],"#0a0","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["closePath",[],"#0a0","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["fill",[],"#0a0","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["restore",[],"#000","#000","10px sans-serif","alphabetic","start",1,"source-over"]
        ]"##;
        let drawn = canvas_ops_png(100, 100, ops).expect("triangle");
        let blank = canvas_ops_png(100, 100, "[]").expect("blank");
        assert_ne!(drawn, blank, "a filled path must change the canvas");
        assert_eq!(png_dims(&drawn), (100, 100));
    }

    #[test]
    fn unknown_ops_and_bad_pixel_sources_do_not_error() {
        let ops = r##"[
            ["setLineDash",[[5,5]],"#000","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["drawImage",["img",0,0,50,50],"#000","#000","10px sans-serif","alphabetic","start",1,"source-over"],
            ["putImageData",["img",0,0],"#000","#000","10px sans-serif","alphabetic","start",1,"source-over"]
        ]"##;
        let png = canvas_ops_png(60, 60, ops).expect("tolerant replay");
        assert_eq!(png_dims(&png), (60, 60));
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(canvas_ops_png(10, 10, "not json").is_err());
    }

    #[test]
    fn color_parsing_covers_the_common_forms() {
        assert_eq!(
            parse_color("#f00"),
            Some(Rgba {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            })
        );
        assert_eq!(
            parse_color("#00ff00"),
            Some(Rgba {
                r: 0,
                g: 255,
                b: 0,
                a: 255
            })
        );
        assert_eq!(
            parse_color("rgb(10, 20, 30)"),
            Some(Rgba {
                r: 10,
                g: 20,
                b: 30,
                a: 255
            })
        );
        assert_eq!(
            parse_color("rgba(0,0,0,0.5)"),
            Some(Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 128
            })
        );
        assert_eq!(
            parse_color("white"),
            Some(Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255
            })
        );
        assert_eq!(parse_color("bogus"), None);
    }

    #[test]
    fn font_shorthand_parsing() {
        let (size, family, weight, italic) = parse_font("bold 16px sans-serif");
        assert_eq!(size, 16.0);
        assert_eq!(family, "sans-serif");
        assert_eq!(weight, 700);
        assert!(!italic);

        let (size, family, _, _) = parse_font("14px 'Arial'");
        assert_eq!(size, 14.0);
        assert_eq!(family, "'Arial'");
    }
}
