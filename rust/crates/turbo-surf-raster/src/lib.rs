//! Synthetic screenshots for turbo-surf: turn an HTML **snapshot** into a PNG or
//! SVG image with no browser and no rendering surface of our own.
//!
//! The engine borrows turbo-html2pdf's native HTML/CSS **layout** (block/inline/
//! flex/table + real font shaping over bundled faces) to turn a raw HTML string
//! into a positioned [`Fragment`] display list, then paints that list two ways:
//! a raster [`paint_png`] (tiny-skia) and a vector [`paint_svg`]. Both walk the
//! same fragments in CSS stacking (paint) order via `Fragment::paint_order`, so
//! they agree.
//!
//! This is a *reasonably representative* render, not a pixel-faithful browser:
//! `position`/`z-index` are honored for block flow (out-of-flow boxes are placed
//! against their containing block and painted back-to-front per CSS 2.2 §9.9),
//! but there is no JS-driven visual state beyond whatever produced the snapshot,
//! and `<img>` bytes are drawn as neutral placeholders (layout has their box, not
//! their pixels). It runs only when asked — never on the fetch/extract hot path.
//!
//! Because the input is just an HTML string, any snapshot works: the initial
//! fetch, or any entry in a hydration trail.

mod glyph;
mod paint_png;
mod paint_svg;
mod style_extract;

pub use style_extract::stylesheet_hrefs;

use turbo_html2pdf_core::text::FontRegistry;
use turbo_html2pdf_core::{layout_html, Diagnostics, Fragment, FragmentContent, Rgba};

/// The layout viewport a snapshot is rendered against. `width` drives CSS layout
/// (line wrapping, `%` widths); the image is `width × height` px and content
/// past the bottom edge is clipped, matching a browser viewport screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    /// A common desktop default (overridable per call / per session).
    pub const DEFAULT: Viewport = Viewport {
        width: 1280,
        height: 800,
    };
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport::DEFAULT
    }
}

/// Output encoding for a screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Svg,
}

/// Lay `html` out at `viewport` and paint it into a PNG. Returns encoded bytes.
pub fn screenshot_png(html: &str, viewport: Viewport) -> Result<Vec<u8>, String> {
    screenshot_png_with_css(html, "", viewport)
}

/// Lay `html` out at `viewport` and paint it into a standalone SVG document.
pub fn screenshot_svg(html: &str, viewport: Viewport) -> Result<String, String> {
    screenshot_svg_with_css(html, "", viewport)
}

/// Like [`screenshot_png`] but with `external_css` (e.g. the concatenated bodies
/// of the page's `<link rel="stylesheet">` sheets, fetched by the caller — the
/// raster does no I/O) cascaded on top of the page's own inline styles.
pub fn screenshot_png_with_css(
    html: &str,
    external_css: &str,
    viewport: Viewport,
) -> Result<Vec<u8>, String> {
    let galley = lay_out(html, external_css, viewport.width)?;
    let bg = canvas_background(&galley, viewport.width);
    paint_png::paint(&galley, viewport.width, viewport.height, bg)
}

/// Like [`screenshot_svg`] but with caller-fetched `external_css` (see
/// [`screenshot_png_with_css`]).
pub fn screenshot_svg_with_css(
    html: &str,
    external_css: &str,
    viewport: Viewport,
) -> Result<String, String> {
    let galley = lay_out(html, external_css, viewport.width)?;
    let bg = canvas_background(&galley, viewport.width);
    Ok(paint_svg::paint(
        &galley,
        viewport.width,
        viewport.height,
        bg,
    ))
}

/// The colour to fill the whole image with before painting. Browsers propagate
/// the root element's (or `<body>`'s) background to the viewport canvas, so a
/// page with a dark `body` background paints dark everywhere — not just under
/// its content box. We approximate that: the first opaque full-width box at the
/// top-left is the root/body, and its background becomes the canvas fill.
/// Defaults to white (the CSS initial canvas colour).
fn canvas_background(galley: &Fragment, width: u32) -> Rgba {
    fn find(f: &Fragment, w: f32) -> Option<Rgba> {
        if let FragmentContent::Box {
            background: Some(bg),
            ..
        } = &f.content
        {
            // A top-left, near-full-width, opaque box = the root/body backdrop.
            if bg.a == 255 && f.x <= 1.0 && f.y <= 1.0 && f.width >= 0.98 * w {
                return Some(*bg);
            }
        }
        f.children.iter().find_map(|c| find(c, w))
    }
    find(galley, width as f32).unwrap_or(Rgba {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    })
}

/// Dispatch by [`Format`]; SVG bytes are UTF-8 of the document.
pub fn screenshot(html: &str, viewport: Viewport, format: Format) -> Result<Vec<u8>, String> {
    match format {
        Format::Png => screenshot_png(html, viewport),
        Format::Svg => screenshot_svg(html, viewport).map(String::into_bytes),
    }
}

/// Drive the borrowed layout engine: recover the page's own `<style>` sheets
/// (html5ever drops `<head>`, so we scrape them from the raw source), strip the
/// elements whose source must not paint (`<script>`/`<style>`/…), and lay the
/// body out at content width `width` over the bundled font set.
fn lay_out(html: &str, external_css: &str, width: u32) -> Result<Fragment, String> {
    // Author CSS order (lowest→highest): external `<link>` sheets the caller
    // fetched, then the page's own `<style>` blocks. Then strip script/style/etc.
    // so their text isn't flowed as visible content.
    let mut author_css = String::from(external_css);
    author_css.push('\n');
    author_css.push_str(&style_extract::collect_style_blocks(html));
    let visible_html = style_extract::strip_non_visual(html);
    let mut diags = Diagnostics::default();
    layout_html(
        &visible_html,
        &author_css,
        width as f32,
        &FontRegistry::new(),
        &mut diags,
    )
    .map_err(|e| format!("layout failed: {e}"))
}
