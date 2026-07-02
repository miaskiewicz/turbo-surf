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
//! and `<img>`/`background-image` bytes the caller supplies are decoded + painted
//! (PNG/JPEG/GIF/WebP + SVG; unknown formats fall back to a placeholder) — but
//! there is no JS-driven visual state beyond whatever produced the snapshot. It
//! runs only when asked — never on the fetch/extract hot path.
//!
//! Because the input is just an HTML string, any snapshot works: the initial
//! fetch, or any entry in a hydration trail.

mod glyph;
mod image_paint;
mod paint_png;
mod paint_svg;
mod style_extract;

pub use style_extract::{image_urls, stylesheet_hrefs};

use std::collections::HashMap;

use turbo_html2pdf_core::text::FontRegistry;
use turbo_html2pdf_core::{
    layout_html_with_images, Diagnostics, Fragment, FragmentContent, ImageCtx, ImageResolver, Rgba,
};

/// Fetched image bytes keyed by their reference — the raw `<img src>` /
/// `background-image: url(...)` value exactly as [`image_urls`] returns it. The
/// caller fetches (the raster does no I/O) and hands the map to a `*_with_assets`
/// screenshot; both layout (intrinsic sizing) and paint read the same bytes.
pub type ImageAssets = HashMap<String, Vec<u8>>;

/// The empty asset map: no image bytes, so `<img>`/`background-image` boxes lay
/// out without pixels (as the `*_with_css` entries do).
fn no_assets() -> ImageAssets {
    HashMap::new()
}

/// An [`ImageResolver`] backed by an [`ImageAssets`] map (verbatim key lookup).
struct AssetResolver<'a>(&'a ImageAssets);

impl ImageResolver for AssetResolver<'_> {
    fn resolve(&self, name: &str) -> Option<&[u8]> {
        self.0.get(name).map(Vec::as_slice)
    }
}

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
    screenshot_png_with_assets(html, external_css, viewport, &no_assets(), false)
}

/// Like [`screenshot_svg`] but with caller-fetched `external_css` (see
/// [`screenshot_png_with_css`]).
pub fn screenshot_svg_with_css(
    html: &str,
    external_css: &str,
    viewport: Viewport,
) -> Result<String, String> {
    screenshot_svg_with_assets(html, external_css, viewport, &no_assets(), false)
}

/// Upper bound on a `full_page` image's height (px) so a pathological page can't
/// allocate an unbounded canvas. ~24k px is far beyond any real page fold.
const MAX_FULL_PAGE_HEIGHT: u32 = 24_000;

/// The image height to paint: the viewport height (viewport-clipped), or — when
/// `full_page` — the laid-out content height (clamped), so nothing below the fold
/// is cut off. Width is always the viewport width.
fn paint_height(galley: &Fragment, viewport: Viewport, full_page: bool) -> u32 {
    if !full_page {
        return viewport.height;
    }
    let content = galley.bottom().ceil().max(1.0) as u32;
    content.min(MAX_FULL_PAGE_HEIGHT)
}

/// Like [`screenshot_png_with_css`] but also paints `<img>` / `background-image`
/// boxes from caller-fetched `images` (keyed as [`image_urls`] returns them; the
/// raster does no I/O). PNG + JPEG are drawn scaled into their layout box; an
/// image whose bytes are absent (or undecodable) lays out without pixels. With
/// `full_page`, the image height is the full content height (viewport width still
/// drives layout) instead of the viewport height.
pub fn screenshot_png_with_assets(
    html: &str,
    external_css: &str,
    viewport: Viewport,
    images: &ImageAssets,
    full_page: bool,
) -> Result<Vec<u8>, String> {
    let decoded = image_paint::decode_all(images);
    let galley = lay_out(
        html,
        external_css,
        viewport.width,
        &image_paint::png_map(&decoded),
    )?;
    let bg = canvas_background(&galley, viewport.width);
    let height = paint_height(&galley, viewport, full_page);
    paint_png::paint(&galley, viewport.width, height, bg, &decoded)
}

/// Like [`screenshot_svg_with_css`] but with caller-fetched `images` (see
/// [`screenshot_png_with_assets`]); images embed as base64 `data:` URIs. With
/// `full_page`, the height is the full content height.
pub fn screenshot_svg_with_assets(
    html: &str,
    external_css: &str,
    viewport: Viewport,
    images: &ImageAssets,
    full_page: bool,
) -> Result<String, String> {
    let decoded = image_paint::decode_all(images);
    let galley = lay_out(
        html,
        external_css,
        viewport.width,
        &image_paint::png_map(&decoded),
    )?;
    let bg = canvas_background(&galley, viewport.width);
    let height = paint_height(&galley, viewport, full_page);
    Ok(paint_svg::paint(
        &galley,
        viewport.width,
        height,
        bg,
        &decoded,
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
fn lay_out(
    html: &str,
    external_css: &str,
    width: u32,
    images: &ImageAssets,
) -> Result<Fragment, String> {
    // Author CSS order (lowest→highest): external `<link>` sheets the caller
    // fetched, then the page's own `<style>` blocks. Then strip script/style/etc.
    // so their text isn't flowed as visible content.
    let mut author_css = String::from(external_css);
    author_css.push('\n');
    author_css.push_str(&style_extract::collect_style_blocks(html));
    let visible_html = style_extract::strip_non_visual(html);
    let mut diags = Diagnostics::default();
    let resolver = AssetResolver(images);
    // No page-height basis in a viewport render — only the 100%-width cap applies
    // (images shrink to fit their column, keeping aspect ratio).
    let image_ctx = ImageCtx {
        resolver: &resolver,
        body_height: None,
    };
    layout_html_with_images(
        &visible_html,
        &author_css,
        width as f32,
        &FontRegistry::new(),
        &image_ctx,
        &mut diags,
    )
    .map_err(|e| format!("layout failed: {e}"))
}
