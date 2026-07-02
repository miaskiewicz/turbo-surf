//! turbo-surf napi-rs addon — the in-process bridge from the Rust core to Node.
//! Stateless functional surface: Node holds the HTML, each call parses it with
//! turbo-dom and runs a Rust view/extract pass. `fetchHtml` + `crawl` are async
//! (driven on napi's tokio runtime). The thin `@playwright/test` shim (task #10)
//! composes these; `goto` = `fetchHtml` then view calls on the cached HTML.

mod session;
// Re-export the live-session napi free functions so they're reachable from the crate
// root (a `pub fn` in a private module is otherwise dead-code-flagged in the lib build;
// napi still registers them either way).
pub use session::{live_close, live_cookies, live_eval, live_open, live_serialize};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use turbo_dom_parser::rtdom::serialize::serialize_inner;
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;
use turbo_surf_core::crawl::{crawl as run_crawl, CrawlOptions, Record};
use turbo_surf_core::net::{build_client, fetch_html as net_fetch, FetchOptions};
use turbo_surf_page::TurboNavigator;
use turbo_surf_raster as raster;
use turbo_surf_view as view;
use view::{Field, FieldType, QueryType, TextMode};

#[napi]
pub fn version() -> String {
    "0.3.2".to_string()
}

fn to_json_string<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".to_string())
}

thread_local! {
    /// Memoize the most recent `html -> Tree` parse per thread. Reads are stateless
    /// (Node passes the HTML each call), so a page that gets several view passes
    /// (links + markdown + text + extract — the real crawl pattern, and the shim's
    /// query-then-accessors) re-parsed the 250 KB document every call. Cache the last
    /// one: a same-HTML follow-up call is a cheap string compare + `Rc` clone instead
    /// of a ~5 ms re-parse. Mutating actions still own their own `Tree` (need `&mut`).
    static PARSE_CACHE: std::cell::RefCell<Option<(String, std::rc::Rc<Tree>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Parsed `Tree` for `html`, reusing the thread's last parse when the HTML is
/// unchanged. Returned as `Rc<Tree>`; `&tree` derefs to `&Tree` at call sites.
fn parsed(html: &str) -> std::rc::Rc<Tree> {
    PARSE_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.as_ref().map(|(k, _)| k.as_str()) != Some(html) {
            *c = Some((html.to_string(), std::rc::Rc::new(Tree::parse(html))));
        }
        c.as_ref().expect("just set").1.clone()
    })
}

// --- view passes (parse cached per page; the shim caches the HTML) ----------

#[napi]
pub fn markdown(html: String, base_url: String) -> String {
    let tree = parsed(&html);
    view::markdown(&tree, tree.root(), &base_url)
}

// --- synthetic screenshots (no browser) --------------------------------------
// Render an HTML snapshot into an image via the native layout+paint tier. The
// viewport width drives CSS layout; `width`/`height` default to the standard
// desktop viewport when omitted, so a caller can pass nothing for the default.

fn viewport(width: Option<u32>, height: Option<u32>) -> raster::Viewport {
    let d = raster::Viewport::DEFAULT;
    raster::Viewport {
        width: width.unwrap_or(d.width),
        height: height.unwrap_or(d.height),
    }
}

/// PNG screenshot of `html` at the given (or default) viewport → `Buffer`.
#[napi]
pub fn screenshot(html: String, width: Option<u32>, height: Option<u32>) -> Result<Buffer> {
    raster::screenshot_png(&html, viewport(width, height))
        .map(Buffer::from)
        .map_err(Error::from_reason)
}

/// SVG screenshot of `html` at the given (or default) viewport → document string.
#[napi]
pub fn screenshot_svg(html: String, width: Option<u32>, height: Option<u32>) -> Result<String> {
    raster::screenshot_svg(&html, viewport(width, height)).map_err(Error::from_reason)
}

/// The `href`s of `html`'s `<link rel="stylesheet">` elements (verbatim). The
/// caller resolves them against the page URL + fetches them for `*WithCss`.
#[napi]
pub fn stylesheet_hrefs(html: String) -> Vec<String> {
    raster::stylesheet_hrefs(&html)
}

/// PNG screenshot of `html` with caller-fetched `external_css` (the concatenated
/// bodies of the page's `<link>` stylesheets) cascaded on top of its inline CSS.
#[napi]
pub fn screenshot_with_css(
    html: String,
    external_css: String,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<Buffer> {
    raster::screenshot_png_with_css(&html, &external_css, viewport(width, height))
        .map(Buffer::from)
        .map_err(Error::from_reason)
}

/// SVG screenshot of `html` with caller-fetched `external_css` → document string.
#[napi]
pub fn screenshot_svg_with_css(
    html: String,
    external_css: String,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<String> {
    raster::screenshot_svg_with_css(&html, &external_css, viewport(width, height))
        .map_err(Error::from_reason)
}

/// Every image reference in `html` (`<img src>` + `background-image: url(...)`),
/// verbatim + de-duplicated (`data:` skipped). The caller resolves each against
/// the page URL, fetches the bytes, and passes them as the `images` map to
/// `*WithAssets`.
#[napi]
pub fn image_urls(html: String) -> Vec<String> {
    raster::image_urls(&html)
}

/// Convert a JS `{ ref: Buffer }` image map into the raster's asset map.
fn to_assets(images: std::collections::HashMap<String, Buffer>) -> raster::ImageAssets {
    images.into_iter().map(|(k, v)| (k, v.to_vec())).collect()
}

/// PNG screenshot of `html` with caller-fetched `external_css` and `images` (a
/// map from each `imageUrls` ref to its fetched bytes). PNG/JPEG images are
/// painted into their layout box; others fall back to a placeholder.
#[napi]
pub fn screenshot_with_assets(
    html: String,
    external_css: String,
    images: std::collections::HashMap<String, Buffer>,
    width: Option<u32>,
    height: Option<u32>,
    full_page: Option<bool>,
) -> Result<Buffer> {
    raster::screenshot_png_with_assets(
        &html,
        &external_css,
        viewport(width, height),
        &to_assets(images),
        full_page.unwrap_or(false),
    )
    .map(Buffer::from)
    .map_err(Error::from_reason)
}

/// SVG screenshot of `html` with caller-fetched `external_css` and `images`;
/// images embed as base64 `data:` URIs. `fullPage` grows the height to the full
/// content height instead of clipping to the viewport. → document string.
#[napi]
pub fn screenshot_svg_with_assets(
    html: String,
    external_css: String,
    images: std::collections::HashMap<String, Buffer>,
    width: Option<u32>,
    height: Option<u32>,
    full_page: Option<bool>,
) -> Result<String> {
    raster::screenshot_svg_with_assets(
        &html,
        &external_css,
        viewport(width, height),
        &to_assets(images),
        full_page.unwrap_or(false),
    )
    .map_err(Error::from_reason)
}

#[napi]
pub fn text(html: String) -> String {
    let tree = parsed(&html);
    view::text(&tree, tree.root())
}

/// Document `<title>` (raw text — the text view skips TITLE, so read it directly).
#[napi]
pub fn title(html: String) -> String {
    let tree = parsed(&html);
    tree.query_selector("title")
        .map(|h| tree.text_content(h).trim().to_string())
        .unwrap_or_default()
}

#[napi]
pub fn html(html: String) -> String {
    let tree = parsed(&html);
    serialize_inner(&tree, tree.root())
}

#[napi]
pub fn links(html: String, base_url: String) -> Vec<String> {
    let tree = parsed(&html);
    view::links(&tree, &base_url)
}

#[napi]
pub fn interactive_elements(html: String, base_url: String) -> String {
    let tree = parsed(&html);
    to_json_string(&view::interactive_elements(&tree, &base_url, true))
}

#[napi]
pub fn accessibility_tree(html: String) -> String {
    let tree = parsed(&html);
    to_json_string(&view::accessibility_tree(&tree))
}

#[napi]
pub fn aria_snapshot(html: String) -> String {
    let tree = parsed(&html);
    match tree.query_selector("body") {
        Some(b) => view::aria_snapshot(&tree, b),
        None => String::new(),
    }
}

#[napi]
pub fn hydration_state(html: String) -> String {
    let tree = parsed(&html);
    to_json_string(&view::extract_hydration_state(&tree))
}

#[napi]
pub fn detect(html: String) -> String {
    let tree = parsed(&html);
    to_json_string(&view::detect_js_required(&tree, None, None))
}

#[napi]
pub fn query(html: String, selector: String, kind: Option<String>) -> String {
    let tree = parsed(&html);
    let ty = match kind.as_deref() {
        Some("css") => QueryType::Css,
        Some("xpath") => QueryType::Xpath,
        _ => QueryType::Auto,
    };
    to_json_string(&view::query(&tree, tree.root(), &selector, ty))
}

/// Locate by `kind` ("role" | "text" | "label") → JSON `[{ node, text }]`.
/// `name` filters a role match by accessible name (substring).
#[napi]
pub fn get_by(
    html: String,
    kind: String,
    value: String,
    name: Option<String>,
    root: Option<String>,
) -> String {
    let tree = parsed(&html);
    let mode_for = |s: &str| {
        if view::locator::is_regex_literal(s) {
            TextMode::Regex
        } else {
            TextMode::Substring
        }
    };
    let nm = name.as_deref().map(|n| (n, mode_for(n)));
    let mut hits = match kind.as_str() {
        "role" => view::by_role(&tree, &value, nm),
        "text" => view::by_text(&tree, &value, mode_for(&value)),
        "label" => view::by_label(&tree, &value, mode_for(&value)),
        _ => Vec::new(),
    };
    // Scope to a parent locator's subtree: keep only hits that are a descendant (or self) of
    // some element matching `root`. Backs `parentLocator.getByRole/getByText/getByLabel(...)`.
    if let Some(root_sel) = root.as_deref().filter(|s| !s.is_empty()) {
        let roots: std::collections::HashSet<Handle> =
            tree.query_selector_all(root_sel).iter().copied().collect();
        if !roots.is_empty() {
            hits.retain(|&h| {
                let mut n = Some(h);
                while let Some(x) = n {
                    if roots.contains(&x) {
                        return true;
                    }
                    n = tree.parent(x);
                }
                false
            });
        }
    }
    // `idx` = the element's position in document order (querySelectorAll('*')), so the
    // Playwright shim can drive a getBy* match in the LIVE isolate by index (these
    // locators have no CSS selector to dispatch through).
    let all: Vec<Handle> = tree.query_selector_all("*").iter().copied().collect();
    let out: Vec<Value> = hits
        .iter()
        .map(|&h| {
            json!({ "node": h.raw(), "text": view::text(&tree, h), "idx": all.iter().position(|&x| x == h) })
        })
        .collect();
    Value::Array(out).to_string()
}

#[napi]
pub fn extract(html: String, base_url: String, schema_json: String) -> Result<String> {
    let schema_value: Value =
        serde_json::from_str(&schema_json).map_err(|e| Error::from_reason(e.to_string()))?;
    let tree = parsed(&html);
    let schema = parse_schema(&schema_value);
    Ok(to_json_string(&view::extract_schema(
        &tree, &schema, &base_url,
    )))
}

fn parse_schema(v: &Value) -> BTreeMap<String, Field> {
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

// --- JS execution (tier 3, deno_core) ---------------------------------------

/// Evaluate `script` against the page's DOM and return its result as a string
/// (Playwright `page.evaluate`-ish; synchronous, no event loop).
#[napi]
pub fn evaluate(html: String, script: String) -> Result<String> {
    turbo_surf_render::ensure_platform();
    turbo_surf_render::run_with_dom(&html, &script).map_err(Error::from_reason)
}

/// Run the page's own `script` over its DOM (promises/await + virtual timers +
/// `document.cookie`/`fetch` against `baseUrl`) and return the hydrated HTML —
/// the no-Chromium render. The V8 isolate is not `Send`, so it runs on a
/// dedicated thread with its own current-thread runtime (only strings cross).
#[napi]
pub fn render(html: String, base_url: String, script: String) -> Result<String> {
    // Init the V8 platform on THIS (Node main) thread before the per-call worker spawns,
    // so the platform parent is the long-lived main thread, not a transient one.
    turbo_surf_render::ensure_platform();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(turbo_surf_render::render_page(&html, &base_url, &script))
    })
    .join()
    .map_err(|_| Error::from_reason("render thread panicked"))?
    .map_err(Error::from_reason)
}

/// A persistent render worker: one long-lived OS thread owning ONE reused tokio
/// current-thread runtime, fed render jobs over a channel. Two wins over `render`'s
/// per-call `thread::spawn` + fresh runtime: (1) the OS thread + tokio runtime are
/// built once, not per page; (2) the render tier's thread-local isolate pool
/// (`render_page_pooled`) lives on this one thread, so the V8 isolate boot is paid
/// once per process instead of once per page. Renders serialize through it — a JS
/// crawl drives pages sequentially, so that's the real access pattern.
type RenderJob = (
    String,
    String,
    String,
    std::sync::mpsc::Sender<Result<String>>,
);

fn render_worker() -> &'static std::sync::Mutex<std::sync::mpsc::Sender<RenderJob>> {
    static WORKER: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<RenderJob>>> =
        std::sync::OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<RenderJob>();
        std::thread::Builder::new()
            .name("turbo-render".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(_) => return,
                };
                while let Ok((html, base, script, reply)) = rx.recv() {
                    let out = rt
                        .block_on(turbo_surf_render::render_page_pooled(
                            &html,
                            &base,
                            &script,
                            turbo_surf_render::DEFAULT_RENDER_BUDGET_MS,
                        ))
                        .map_err(Error::from_reason);
                    let _ = reply.send(out);
                }
            })
            .expect("spawn render worker");
        std::sync::Mutex::new(tx)
    })
}

/// Like [`render`] but routed to the persistent render worker (reused thread + tokio
/// runtime + pooled V8 isolate). Same string-in/string-out contract; the isolate is a
/// true isolate per page, with cross-page page-JS isolation relaxed for crawl speed.
#[napi]
pub fn render_pooled(html: String, base_url: String, script: String) -> Result<String> {
    // Parent the V8 platform on the main thread before the persistent worker spins up.
    turbo_surf_render::ensure_platform();
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    render_worker()
        .lock()
        .map_err(|_| Error::from_reason("render worker poisoned"))?
        .send((html, base_url, script, reply_tx))
        .map_err(|_| Error::from_reason("render worker gone"))?;
    reply_rx
        .recv()
        .map_err(|_| Error::from_reason("render worker dropped reply"))?
}

/// Hydrate a page by running ITS OWN scripts (inline + dynamically-injected chunks)
/// the way a browser does — backs a JS-mode `goto` that drives a real SPA bundle to
/// mount, with no caller-side script concatenation.
///
/// ASYNC (returns a Promise): hydration fetches chunks over the network, and a same
/// process server (a test's localhost app) can only answer while Node's event loop is
/// free — so this must NOT block it. The render runs on a libuv worker thread (its own
/// current-thread tokio runtime + the non-`Send` V8 isolate, both created and dropped
/// there), and the Promise resolves with the hydrated HTML.
pub struct HydrateTask {
    html: String,
    base_url: String,
    cookies: String,
    user_agent: String,
}

#[napi]
impl Task for HydrateTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::from_reason(e.to_string()))?;
        rt.block_on(turbo_surf_render::render_hydrate_with_budget(
            &self.html,
            &self.base_url,
            &self.cookies,
            &self.user_agent,
            turbo_surf_render::DEFAULT_RENDER_BUDGET_MS,
        ))
        .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: String) -> Result<String> {
        Ok(output)
    }
}

#[napi]
pub fn hydrate(
    html: String,
    base_url: String,
    cookies: Option<String>,
    user_agent: Option<String>,
) -> AsyncTask<HydrateTask> {
    // Parent the V8 platform on the main thread before the libuv worker runs compute().
    turbo_surf_render::ensure_platform();
    AsyncTask::new(HydrateTask {
        html,
        base_url,
        cookies: cookies.unwrap_or_default(),
        user_agent: user_agent.unwrap_or_default(),
    })
}

/// Transform TS/JSX page source → classic JS (swc) so a bundle written in
/// TS/JSX runs under the render tier. Pass `ts`/`jsx` for the input syntax.
#[napi]
pub fn transform(src: String, ts: bool, jsx: bool) -> Result<String> {
    turbo_surf_transform::transform(&src, ts, jsx).map_err(Error::from_reason)
}

/// Render a TS/JSX bundle: transform → run over the DOM → hydrated HTML.
#[napi]
pub fn render_ts(
    html: String,
    base_url: String,
    src: String,
    ts: bool,
    jsx: bool,
) -> Result<String> {
    let script = turbo_surf_transform::transform(&src, ts, jsx).map_err(Error::from_reason)?;
    render(html, base_url, script)
}

// --- per-element accessors (by node handle; back the shim Locator) ----------
// Handles are stable for a given HTML parse, so the shim resolves matches (which
// carry `node`) then reads by handle — works for both `query` and `getBy`.

#[napi]
pub fn attr_of(html: String, node: u32, name: String) -> Option<String> {
    let node = Handle::from_raw(node);
    parsed(&html).get_attribute(node, &name).map(str::to_string)
}

#[napi]
pub fn input_value_of(html: String, node: u32) -> String {
    let node = Handle::from_raw(node);
    view::input_value_of(&parsed(&html), node)
}

#[napi]
pub fn is_visible(html: String, node: u32) -> bool {
    let node = Handle::from_raw(node);
    view::is_visible(&parsed(&html), node)
}

#[napi]
pub fn is_checked(html: String, node: u32) -> bool {
    let node = Handle::from_raw(node);
    view::is_checked(&parsed(&html), node)
}

#[napi]
pub fn is_enabled(html: String, node: u32) -> bool {
    let node = Handle::from_raw(node);
    view::is_enabled(&parsed(&html), node)
}

#[napi]
pub fn is_editable(html: String, node: u32) -> bool {
    let node = Handle::from_raw(node);
    view::is_editable(&parsed(&html), node)
}

#[napi]
pub fn is_empty(html: String, node: u32) -> bool {
    let node = Handle::from_raw(node);
    view::is_empty(&parsed(&html), node)
}

#[napi]
pub fn aria_role_of(html: String, node: u32) -> String {
    let node = Handle::from_raw(node);
    view::role_of(&parsed(&html), node)
}

#[napi]
pub fn accessible_name_of(html: String, node: u32) -> String {
    let node = Handle::from_raw(node);
    view::accessible_name(&parsed(&html), node)
}

#[napi]
pub fn accessible_description_of(html: String, node: u32) -> String {
    let node = Handle::from_raw(node);
    view::accessible_description(&parsed(&html), node)
}

#[napi]
pub fn selected_values_of(html: String, node: u32) -> Vec<String> {
    let node = Handle::from_raw(node);
    view::selected_values(&parsed(&html), node)
}

#[napi]
pub fn css_value_of(html: String, node: u32, name: String) -> String {
    let node = Handle::from_raw(node);
    view::css_value(&parsed(&html), node, &name)
}

/// Whether `expected` is an ordered ARIA-snapshot subset of `node`'s subtree.
#[napi]
pub fn matches_aria_snapshot(html: String, node: u32, expected: String) -> bool {
    let node = Handle::from_raw(node);
    view::matches_aria_snapshot(&parsed(&html), node, &expected)
}

/// Batch read every state-machine accessor for one node in a SINGLE crossing —
/// backs the shim's `expect(locator)` chain so `toBeVisible()` + `toHaveText()` +
/// `toBeEnabled()` marshal the HTML once instead of three times. Returns JSON
/// `{visible,checked,enabled,editable,empty,text,value,role,name,description}`.
/// Attribute/class/CSS matchers stay per-name (no attr-map iterator on the seam).
#[napi]
pub fn node_snapshot(html: String, node: u32) -> String {
    let node = Handle::from_raw(node);
    let tree = parsed(&html);
    json!({
        "visible": view::is_visible(&tree, node),
        "checked": view::is_checked(&tree, node),
        "enabled": view::is_enabled(&tree, node),
        "editable": view::is_editable(&tree, node),
        "empty": view::is_empty(&tree, node),
        "text": view::text(&tree, node),
        "value": view::input_value_of(&tree, node),
        "role": view::role_of(&tree, node),
        "name": view::accessible_name(&tree, node),
        "description": view::accessible_description(&tree, node),
    })
    .to_string()
}

// --- actions (Lane A intent graph) ------------------------------------------
// Each mutating action parses the HTML, mutates the tree, and returns the new
// serialized HTML; the shim swaps its cached HTML for the result.

fn first_match(tree: &Tree, selector: &str) -> Result<Handle> {
    tree.query_selector(selector)
        .ok_or_else(|| Error::from_reason(format!("no element matches {selector}")))
}

/// Fill a control's value (checkbox/radio toggle on non-empty) → new HTML.
#[napi]
pub fn fill(html: String, selector: String, value: String) -> Result<String> {
    let mut tree = Tree::parse(&html);
    let h = first_match(&tree, &selector)?;
    view::fill_value(&mut tree, h, &value);
    Ok(serialize_inner(&tree, tree.root()))
}

/// Set/clear a checkbox/radio's checked state → new HTML.
#[napi]
pub fn set_checked(html: String, selector: String, on: bool) -> Result<String> {
    let mut tree = Tree::parse(&html);
    let h = first_match(&tree, &selector)?;
    view::set_checked(&mut tree, h, on);
    Ok(serialize_inner(&tree, tree.root()))
}

/// Select `<option>`(s) of a `<select>` by value/label → new HTML.
#[napi]
pub fn select_option(html: String, selector: String, value: String) -> Result<String> {
    let mut tree = Tree::parse(&html);
    let h = first_match(&tree, &selector)?;
    view::select_option(&mut tree, h, &value);
    Ok(serialize_inner(&tree, tree.root()))
}

fn intent_json(intent: view::ClickIntent) -> String {
    match intent {
        view::ClickIntent::Navigate(url) => json!({ "action": "navigate", "url": url }).to_string(),
        view::ClickIntent::Submit(s) => json!({
            "action": "submit", "method": s.method, "url": s.url,
            "body": s.body, "contentType": s.content_type,
        })
        .to_string(),
        view::ClickIntent::Inert => json!({ "action": "inert" }).to_string(),
    }
}

/// Resolve what clicking the first `selector` match does (no JS): JSON
/// `{action:"navigate",url}` / `{action:"submit",...}` / `{action:"inert"}`.
#[napi]
pub fn click(html: String, selector: String, base_url: String) -> Result<String> {
    let tree = parsed(&html);
    let h = first_match(&tree, &selector)?;
    Ok(intent_json(view::click_intent(&tree, h, &base_url)))
}

// Node-handle action variants (back locator-scoped actions; work for getBy too).

#[napi]
pub fn fill_node(html: String, node: u32, value: String) -> String {
    let node = Handle::from_raw(node);
    let mut tree = Tree::parse(&html);
    view::fill_value(&mut tree, node, &value);
    serialize_inner(&tree, tree.root())
}

#[napi]
pub fn set_checked_node(html: String, node: u32, on: bool) -> String {
    let node = Handle::from_raw(node);
    let mut tree = Tree::parse(&html);
    view::set_checked(&mut tree, node, on);
    serialize_inner(&tree, tree.root())
}

#[napi]
pub fn select_option_node(html: String, node: u32, value: String) -> String {
    let node = Handle::from_raw(node);
    let mut tree = Tree::parse(&html);
    view::select_option(&mut tree, node, &value);
    serialize_inner(&tree, tree.root())
}

#[napi]
pub fn click_node(html: String, node: u32, base_url: String) -> String {
    let node = Handle::from_raw(node);
    intent_json(view::click_intent(&parsed(&html), node, &base_url))
}

// --- async: fetch + crawl ---------------------------------------------------

/// Process-shared HTTP client so connections + TLS sessions are pooled across
/// every `fetchHtml`/`request`/`goto` (without it, each fetch paid a fresh TLS
/// handshake — the dominant per-page cost in a multi-page crawl).
fn shared_client() -> &'static turbo_surf_core::http_backend::Client {
    static CLIENT: std::sync::OnceLock<turbo_surf_core::http_backend::Client> =
        std::sync::OnceLock::new();
    CLIENT.get_or_init(build_client)
}

async fn do_fetch(
    url: &str,
    method: Option<String>,
    body: Option<String>,
    cookies: Option<String>,
) -> Result<String> {
    do_fetch_headers(url, method, body, cookies, None).await
}

/// `do_fetch` plus extra request headers (JSON object) — backs the shim's
/// `page.setExtraHTTPHeaders` / context `extraHTTPHeaders` (real, not a no-op).
async fn do_fetch_headers(
    url: &str,
    method: Option<String>,
    body: Option<String>,
    cookies: Option<String>,
    headers_json: Option<String>,
) -> Result<String> {
    let mut jar = cookies
        .as_deref()
        .map(turbo_surf_core::cookies::CookieJar::from_storage_state);
    let headers = headers_json
        .as_deref()
        .and_then(|h| serde_json::from_str::<BTreeMap<String, String>>(h).ok())
        .unwrap_or_default();
    let opts = FetchOptions {
        method,
        body,
        headers,
        allow_non_html: true,
        jar: jar.as_mut(),
        client: Some(shared_client()),
        ..Default::default()
    };
    let res = net_fetch(url, opts)
        .await
        .map_err(|e| Error::from_reason(e.to_string()))?;
    let cookie_state = jar
        .as_ref()
        .map(|j| j.storage_state())
        .unwrap_or_else(|| "[]".to_string());
    Ok(json!({
        "html": res.html,
        "finalUrl": res.final_url,
        "status": res.status,
        "redirected": res.redirected,
        "contentType": res.content_type,
        "cookies": cookie_state,
    })
    .to_string())
}

/// Fetch a URL (GET); returns JSON `{ html, finalUrl, status, redirected, cookies }`.
#[napi]
pub async fn fetch_html(url: String) -> Result<String> {
    do_fetch(&url, None, None, None).await
}

/// Fetch a URL as raw bytes (no charset decode) → a `Buffer`. For binary
/// resources — images for `screenshotWithAssets` — that a `fetchHtml` string
/// would corrupt. Uses the shared pooled client; non-HTML content types allowed.
#[napi]
pub async fn fetch_bytes(url: String) -> Result<Buffer> {
    let opts = FetchOptions {
        allow_non_html: true,
        client: Some(shared_client()),
        ..Default::default()
    };
    let (_final_url, bytes, _status) = turbo_surf_core::net::fetch_bytes(&url, opts)
        .await
        .map_err(|e| Error::from_reason(e.to_string()))?;
    Ok(Buffer::from(bytes))
}

/// Fetch with an explicit method/body (e.g. a POST form submission).
#[napi]
pub async fn request(url: String, method: String, body: Option<String>) -> Result<String> {
    do_fetch(&url, Some(method), body, None).await
}

/// Fetch carrying a `storageState` cookie string in, and the updated state out
/// (Set-Cookie ingested) — cookie persistence across navigations.
#[napi]
pub async fn fetch_with_cookies(
    url: String,
    cookies: String,
    method: Option<String>,
    body: Option<String>,
    headers: Option<String>,
) -> Result<String> {
    do_fetch_headers(&url, method, body, Some(cookies), headers).await
}

fn record_json(r: &Record) -> Value {
    json!({
        "url": r.url, "status": r.status, "depth": r.depth,
        "title": r.title, "links": r.links, "error": r.error, "items": r.items,
    })
}

fn crawl_options(opts: &Value) -> CrawlOptions {
    let start = opts
        .get("start")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let u = |k: &str, d: u64| opts.get(k).and_then(Value::as_u64).unwrap_or(d);
    let concurrency = u("concurrency", 4) as usize;
    CrawlOptions {
        start,
        max_pages: u("maxPages", 100) as usize,
        max_depth: u("maxDepth", 3) as usize,
        concurrency,
        // default per-host to the global cap; politeness off unless asked. These let
        // a benchmark match a competitor's fairness settings exactly.
        per_host_concurrency: u("perHostConcurrency", concurrency as u64) as usize,
        politeness_ms: u("politenessMs", 0),
        same_host_only: opts
            .get("sameHost")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        ..Default::default()
    }
}

/// Crawl from `optsJson` (`{ start:[…], maxPages?, maxDepth?, concurrency?,
/// sameHost?, itemSelector? }`); returns a JSON array of page records. When
/// `itemSelector` is set, each record's `items` is the count of matching elements
/// on that page (extraction-during-crawl, for the crawler benchmark's parity metric).
#[napi]
pub async fn crawl(opts_json: String) -> Result<String> {
    let opts_value: Value =
        serde_json::from_str(&opts_json).map_err(|e| Error::from_reason(e.to_string()))?;
    let opts = crawl_options(&opts_value);
    let item_selector = opts_value
        .get("itemSelector")
        .and_then(Value::as_str)
        .map(str::to_string);
    let nav = TurboNavigator::default().with_item_selector(item_selector);
    let recs = run_crawl(opts, Arc::new(nav)).await;
    let arr: Vec<Value> = recs.iter().map(record_json).collect();
    Ok(Value::Array(arr).to_string())
}
