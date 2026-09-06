//! turbo-surf PyO3 binding (PyPI: `turbo-surf`).
//!
//! Exposes the browserless parse → view/extract → JS-render surface of the Rust
//! engine to Python, mirroring the Node N-API binding's stateless functions 1:1:
//! pass a page's HTML in, get a view (markdown/text/links/…), a typed extraction,
//! or — for JS-gated pages — the hydrated HTML after the page's own scripts run in
//! the V8 isolate. No headless browser.
//!
//! ## Boundary contract
//! * Inputs are plain `str` (the page HTML, a CSS/XPath selector, a base URL).
//! * Outputs are `str` (often JSON, as in the N-API surface) — the caller parses
//!   the JSON shapes documented per function.
//! * Fatal faults (bad schema JSON, a render-tier failure) raise
//!   `TurboSurfError`; non-JS views never raise.
//!
//! The product surface is this thin marshaling layer; all logic lives in the
//! `turbo-surf-view` / `turbo-surf-render` crates. This crate is a cdylib kept
//! deliberately minimal and mechanical.

#![deny(clippy::all)]

mod errors;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use serde_json::Value;
use turbo_dom_parser::rtdom::serialize::serialize_inner;
use turbo_dom_parser::rtdom::Tree;
use turbo_surf_raster as raster;
use turbo_surf_view as view;
use view::{Field, FieldType, QueryType};

use errors::TurboSurfError;

fn to_json_string<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".to_string())
}

/// The package version (matches the crate / wheel version).
#[pyfunction]
fn version() -> &'static str {
    "0.4.1"
}

// --- view passes (parse + read; one parse per call) -------------------------

/// Render the page to Markdown (links resolved against `base_url`).
#[pyfunction]
#[pyo3(signature = (html, base_url=String::new()))]
fn markdown(html: &str, base_url: String) -> String {
    let tree = Tree::parse(html);
    view::markdown(&tree, tree.root(), &base_url)
}

/// The page's visible text (block-aware).
#[pyfunction]
fn text(html: &str) -> String {
    let tree = Tree::parse(html);
    view::text(&tree, tree.root())
}

// --- synthetic screenshots (no browser) --------------------------------------

fn viewport(width: Option<u32>, height: Option<u32>) -> raster::Viewport {
    let d = raster::Viewport::DEFAULT;
    raster::Viewport {
        width: width.unwrap_or(d.width),
        height: height.unwrap_or(d.height),
    }
}

/// A synthetic PNG screenshot of `html` — native layout + paint, no browser.
/// `width`/`height` default to a 1280×800 viewport. Returns PNG `bytes`.
#[pyfunction]
#[pyo3(signature = (html, width=None, height=None))]
fn screenshot<'py>(
    py: Python<'py>,
    html: &str,
    width: Option<u32>,
    height: Option<u32>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes =
        raster::screenshot_png(html, viewport(width, height)).map_err(TurboSurfError::new_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// A synthetic SVG screenshot of `html` (no browser) → the SVG document `str`.
#[pyfunction]
#[pyo3(signature = (html, width=None, height=None))]
fn screenshot_svg(html: &str, width: Option<u32>, height: Option<u32>) -> PyResult<String> {
    raster::screenshot_svg(html, viewport(width, height)).map_err(TurboSurfError::new_err)
}

/// The `href`s of `html`'s `<link rel="stylesheet">` elements (verbatim). Fetch
/// them yourself (resolving against the page URL) and pass the concatenated CSS
/// to `screenshot_with_css` for a styled render of an external-CSS page.
#[pyfunction]
fn stylesheet_hrefs(html: &str) -> Vec<String> {
    raster::stylesheet_hrefs(html)
}

/// PNG screenshot of `html` with caller-fetched `external_css` cascaded on top
/// of the page's inline styles. Returns PNG `bytes`.
#[pyfunction]
#[pyo3(signature = (html, external_css, width=None, height=None))]
fn screenshot_with_css<'py>(
    py: Python<'py>,
    html: &str,
    external_css: &str,
    width: Option<u32>,
    height: Option<u32>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = raster::screenshot_png_with_css(html, external_css, viewport(width, height))
        .map_err(TurboSurfError::new_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// SVG screenshot of `html` with caller-fetched `external_css` → document `str`.
#[pyfunction]
#[pyo3(signature = (html, external_css, width=None, height=None))]
fn screenshot_svg_with_css(
    html: &str,
    external_css: &str,
    width: Option<u32>,
    height: Option<u32>,
) -> PyResult<String> {
    raster::screenshot_svg_with_css(html, external_css, viewport(width, height))
        .map_err(TurboSurfError::new_err)
}

/// Every image reference in `html` (`<img src>` + `background-image: url(...)`),
/// verbatim + de-duplicated (`data:` skipped). Fetch each yourself (resolving
/// against the page URL) and pass a `{ref: bytes}` dict to `*_with_assets`.
#[pyfunction]
fn image_urls(html: &str) -> Vec<String> {
    raster::image_urls(html)
}

/// PNG screenshot of `html` with caller-fetched `external_css` and `images` (a
/// `{ref: bytes}` dict from `image_urls`). PNG/JPEG images are painted; others
/// fall back to a placeholder. Returns PNG `bytes`.
#[pyfunction]
#[pyo3(signature = (html, external_css, images, width=None, height=None, full_page=false))]
fn screenshot_with_assets<'py>(
    py: Python<'py>,
    html: &str,
    external_css: &str,
    images: std::collections::HashMap<String, Vec<u8>>,
    width: Option<u32>,
    height: Option<u32>,
    full_page: bool,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = raster::screenshot_png_with_assets(
        html,
        external_css,
        viewport(width, height),
        &images,
        full_page,
    )
    .map_err(TurboSurfError::new_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// SVG screenshot of `html` with caller-fetched `external_css` and `images`
/// (images embed as base64 `data:` URIs) → document `str`. `full_page` grows the
/// height to the full content height instead of clipping to the viewport.
#[pyfunction]
#[pyo3(signature = (html, external_css, images, width=None, height=None, full_page=false))]
fn screenshot_svg_with_assets(
    html: &str,
    external_css: &str,
    images: std::collections::HashMap<String, Vec<u8>>,
    width: Option<u32>,
    height: Option<u32>,
    full_page: bool,
) -> PyResult<String> {
    raster::screenshot_svg_with_assets(
        html,
        external_css,
        viewport(width, height),
        &images,
        full_page,
    )
    .map_err(TurboSurfError::new_err)
}

/// The document `<title>` (trimmed; empty if none).
#[pyfunction]
fn title(html: &str) -> String {
    let tree = Tree::parse(html);
    tree.query_selector("title")
        .map(|h| tree.text_content(h).trim().to_string())
        .unwrap_or_default()
}

/// The page serialized back to HTML.
#[pyfunction]
fn html(html: &str) -> String {
    let tree = Tree::parse(html);
    serialize_inner(&tree, tree.root())
}

/// All hyperlink targets, resolved against `base_url`.
#[pyfunction]
#[pyo3(signature = (html, base_url=String::new()))]
fn links(html: &str, base_url: String) -> Vec<String> {
    let tree = Tree::parse(html);
    view::links(&tree, &base_url)
}

/// JSON array of the page's interactive elements (links/buttons/inputs), with
/// hrefs resolved against `base_url`.
#[pyfunction]
#[pyo3(signature = (html, base_url=String::new()))]
fn interactive_elements(html: &str, base_url: String) -> String {
    let tree = Tree::parse(html);
    to_json_string(&view::interactive_elements(&tree, &base_url, true))
}

/// JSON accessibility tree for the document.
#[pyfunction]
fn accessibility_tree(html: &str) -> String {
    let tree = Tree::parse(html);
    to_json_string(&view::accessibility_tree(&tree))
}

/// JSON hydration-state probe (does the page need JS to render content?).
#[pyfunction]
fn hydration_state(html: &str) -> String {
    let tree = Tree::parse(html);
    to_json_string(&view::extract_hydration_state(&tree))
}

/// JSON verdict on whether the page is JS-gated.
#[pyfunction]
fn detect(html: &str) -> String {
    let tree = Tree::parse(html);
    to_json_string(&view::detect_js_required(&tree, None, None))
}

/// Query the DOM — `kind` is `"css"`, `"xpath"`, or omitted for auto-detect.
/// Returns a JSON array of matches.
#[pyfunction]
#[pyo3(signature = (html, selector, kind=None))]
fn query(html: &str, selector: &str, kind: Option<&str>) -> String {
    let tree = Tree::parse(html);
    let ty = match kind {
        Some("css") => QueryType::Css,
        Some("xpath") => QueryType::Xpath,
        _ => QueryType::Auto,
    };
    to_json_string(&view::query(&tree, tree.root(), selector, ty))
}

/// Structured extraction: `schema_json` maps field names to selector specs;
/// returns the extracted records as a JSON object. Raises `TurboSurfError` if
/// the schema JSON is malformed.
#[pyfunction]
#[pyo3(signature = (html, schema_json, base_url=String::new()))]
fn extract(html: &str, schema_json: &str, base_url: String) -> PyResult<String> {
    let schema_value: Value = serde_json::from_str(schema_json)
        .map_err(|e| TurboSurfError::new_err(format!("invalid schema JSON: {e}")))?;
    let tree = Tree::parse(html);
    let schema = parse_schema(&schema_value);
    Ok(to_json_string(&view::extract_schema(
        &tree, &schema, &base_url,
    )))
}

fn parse_schema(v: &Value) -> std::collections::BTreeMap<String, Field> {
    v.as_object()
        .map(|o| o.iter().map(|(k, s)| (k.clone(), parse_field(s))).collect())
        .unwrap_or_default()
}

fn parse_field(spec: &Value) -> Field {
    let s = |k: &str| spec.get(k).and_then(Value::as_str).map(str::to_string);
    Field {
        selector: s("selector"),
        attr: s("attr"),
        ftype: match spec.get("type").and_then(Value::as_str) {
            Some("number") => FieldType::Number,
            Some("boolean") => FieldType::Boolean,
            _ => FieldType::String,
        },
        list: spec.get("list").and_then(Value::as_bool).unwrap_or(false),
        fields: spec.get("fields").map(parse_schema),
    }
}

// --- JS execution tier (deno_core V8) ---------------------------------------

/// Evaluate `script` against the page's DOM and return its result as a string
/// (Playwright `page.evaluate`-ish; synchronous, no event loop).
#[pyfunction]
fn evaluate(html: &str, script: &str) -> PyResult<String> {
    turbo_surf_render::ensure_platform();
    turbo_surf_render::run_with_dom(html, script).map_err(TurboSurfError::new_err)
}

/// Run the page's own `script` over its DOM (promises/await + virtual timers +
/// `document.cookie`/`fetch` against `base_url`) and return the hydrated HTML —
/// the no-Chromium render. The V8 isolate is not `Send`, so it runs on a
/// dedicated thread with its own current-thread runtime (only strings cross).
#[pyfunction]
#[pyo3(signature = (html, script, base_url=String::new()))]
fn render(py: Python<'_>, html: &str, script: &str, base_url: String) -> PyResult<String> {
    turbo_surf_render::ensure_platform();
    let (html, script, base) = (html.to_string(), script.to_string(), base_url);
    // Release the GIL while the render tier runs the page's scripts on its own
    // thread — long renders shouldn't block other Python threads.
    py.allow_threads(move || {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            rt.block_on(turbo_surf_render::render_page(&html, &base, &script))
        })
        .join()
        .map_err(|_| "render thread panicked".to_string())?
    })
    .map_err(TurboSurfError::new_err)
}

/// Transform TS/JSX source → classic JS (swc), so a TS/JSX page bundle can run
/// under the render tier. Raises `TurboSurfError` on a transform failure.
#[pyfunction]
#[pyo3(signature = (src, ts=false, jsx=false))]
fn transform(src: &str, ts: bool, jsx: bool) -> PyResult<String> {
    turbo_surf_transform::transform(src, ts, jsx).map_err(TurboSurfError::new_err)
}

/// Register module-level functions.
fn register_functions(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(markdown, m)?)?;
    m.add_function(wrap_pyfunction!(text, m)?)?;
    m.add_function(wrap_pyfunction!(screenshot, m)?)?;
    m.add_function(wrap_pyfunction!(screenshot_svg, m)?)?;
    m.add_function(wrap_pyfunction!(stylesheet_hrefs, m)?)?;
    m.add_function(wrap_pyfunction!(screenshot_with_css, m)?)?;
    m.add_function(wrap_pyfunction!(screenshot_svg_with_css, m)?)?;
    m.add_function(wrap_pyfunction!(image_urls, m)?)?;
    m.add_function(wrap_pyfunction!(screenshot_with_assets, m)?)?;
    m.add_function(wrap_pyfunction!(screenshot_svg_with_assets, m)?)?;
    m.add_function(wrap_pyfunction!(title, m)?)?;
    m.add_function(wrap_pyfunction!(html, m)?)?;
    m.add_function(wrap_pyfunction!(links, m)?)?;
    m.add_function(wrap_pyfunction!(interactive_elements, m)?)?;
    m.add_function(wrap_pyfunction!(accessibility_tree, m)?)?;
    m.add_function(wrap_pyfunction!(hydration_state, m)?)?;
    m.add_function(wrap_pyfunction!(detect, m)?)?;
    m.add_function(wrap_pyfunction!(query, m)?)?;
    m.add_function(wrap_pyfunction!(extract, m)?)?;
    m.add_function(wrap_pyfunction!(evaluate, m)?)?;
    m.add_function(wrap_pyfunction!(render, m)?)?;
    m.add_function(wrap_pyfunction!(transform, m)?)
}

/// The Python extension module `turbo_surf._turbo_surf`. Re-exported by the
/// pure-Python `turbo_surf/__init__.py` shim.
#[pymodule]
fn _turbo_surf(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_functions(m)?;
    m.add("TurboSurfError", m.py().get_type::<TurboSurfError>())
}
