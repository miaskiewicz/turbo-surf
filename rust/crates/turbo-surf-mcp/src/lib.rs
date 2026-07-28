//! turbo-surf MCP server core (port of `mcp/`) — a stateful agent session over
//! a current page `Tree`, exposed via stdio JSON-RPC 2.0. No Node, no SDK: the
//! JSON-RPC envelope is hand-rolled (`initialize` / `tools/list` / `tools/call`).
//!
//! `goto` fetches + parses into the session; the read tools (markdown / text /
//! html / links / interactive_elements / accessibility_tree / aria_snapshot /
//! extract / hydration_state / query / get_by / detect) run over that `Tree`.
//! Action tools (click/fill/submit) need the navigation state machine and land
//! with the tier-2 `Page` wiring.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use turbo_dom_parser::rtdom::serialize::serialize_inner;
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;
use turbo_surf_core::challenge::{self, ChallengeSolver, SolveContext};
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_core::crawl::{crawl as run_crawl, CrawlOptions};
use turbo_surf_core::fingerprint;
use turbo_surf_core::net::{fetch_bytes, fetch_html, FetchOptions};
use turbo_surf_core::robots::{RobotsCache, RobotsFetcher};
use turbo_surf_page::{batch as batch_urls, TurboNavigator};
use turbo_surf_raster as raster;
use turbo_surf_view as view;
use view::{Field, FieldType, QueryType, TextMode};

pub const VERSION: &str = "0.4.0";

/// One agent session: the current page URL + parsed tree + nav history, plus the
/// browser-ish state agents expect (UA / extra headers / cookie jar / JS mode) and
/// the trails the JS server exposes (rendered-DOM history + a request log).
#[derive(Default)]
pub struct Session {
    pub url: String,
    tree: Option<Tree>,
    back: Vec<String>,
    forward: Vec<String>,
    ua: Option<String>,
    headers: BTreeMap<String, String>,
    jar: CookieJar,
    /// "" / "no-js" = Lane A; "fast" / "secure" / "js" = render page JS after fetch.
    mode: String,
    /// Hydrated-HTML trail (one entry per render/inject), newest last.
    dom_history: Vec<String>,
    /// Every URL fetched this session (navigations + direct fetches).
    requests: Vec<String>,
    /// Optional challenge solver (Hyper/Scrapfly), configured from env / `.env`.
    /// `None` (the default) leaves the solve path inert.
    solver: Option<Box<dyn ChallengeSolver>>,
    /// Render-tier navigator fingerprint overrides (JSON object), applied via
    /// `set_fingerprint`. Empty = Chrome 149 defaults.
    fingerprint: String,
    /// Layout viewport for `screenshot` (and any future geometry). Defaults to a
    /// common desktop size; overridable via `set_viewport` or per-call args.
    viewport: raster::Viewport,
    /// Seed consent-wall cookies (google/youtube "before you continue") so the
    /// real page is served instead of a JS-gated interstitial. `None` = the
    /// default (on); set `Some(false)` to send the raw consent-gated response.
    bypass_consent: Option<bool>,
    /// Session-scoped `web_search` parse-strategy overrides, keyed by engine id — the
    /// highest-precedence registry layer (see `resolve_strategy`). Populated by
    /// `web_search_load_strategy`; empty by default (falls through to user-dir/built-in).
    search_overrides: HashMap<String, Strategy>,
    /// Session default engine for `web_search` when a call omits `engine` — set via
    /// `web_search_set_engine`. `None` (the default) falls through to `"duckduckgo"`.
    default_search_engine: Option<String>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            // Pick up a solver from env/`.env` if one is configured (else inert).
            // Supply the V8 engine so the Cloudflare solver runs the challenge's own
            // JS to compute the answer (the proper path) instead of the placeholder.
            solver: challenge::solver_from_env_pow(Some(Box::new(turbo_surf_render::V8PowEngine))),
            ..Self::default()
        }
    }

    /// Construct with an explicit solver (used by the e2e harness to inject a
    /// fake/sidecar solver without touching process env).
    pub fn with_solver(solver: Box<dyn ChallengeSolver>) -> Self {
        Self {
            solver: Some(solver),
            ..Self::default()
        }
    }

    // Stable per-host Chrome identity from the seed pool: same host → same
    // profile (so any solver token stays consistent with the fingerprint),
    // distinct hosts spread across the pool.
    fn profile_for(&self, url: &str) -> fingerprint::Profile {
        let key = turbo_surf_core::url::host_of(url).unwrap_or_else(|| url.to_string());
        fingerprint::select(&key)
    }

    /// Inject a parsed tree (test seam, bypasses the network).
    pub fn load(&mut self, url: &str, html: &str) {
        self.url = url.to_string();
        self.tree = Some(Tree::parse(html));
    }

    fn tree(&self) -> Result<&Tree, String> {
        self.tree
            .as_ref()
            .ok_or_else(|| "no page loaded (call goto first)".to_string())
    }

    fn tree_mut(&mut self) -> Result<&mut Tree, String> {
        self.tree
            .as_mut()
            .ok_or_else(|| "no page loaded (call goto first)".to_string())
    }

    // Headers to send: the configured extra headers + the UA (if set).
    fn request_headers(&self) -> BTreeMap<String, String> {
        let mut h = self.headers.clone();
        if let Some(ua) = &self.ua {
            h.insert("user-agent".to_string(), ua.clone());
        }
        h
    }

    // Fetch + parse into the session (UA / headers / cookie jar applied; the URL is
    // logged; the page's own JS is rendered when a JS mode is active).
    async fn fetch_into(
        &mut self,
        url: &str,
        method: Option<String>,
        body: Option<String>,
    ) -> Result<Value, String> {
        self.requests.push(url.to_string());
        let profile = self.profile_for(url);
        let opts = FetchOptions {
            method,
            body,
            allow_non_html: true,
            headers: self.request_headers(),
            jar: Some(&mut self.jar),
            profile: Some(&profile),
            bypass_consent: self.bypass_consent.unwrap_or(true),
            ..Default::default()
        };
        let res = fetch_html_with(url, opts).await?;
        self.load(&res.0, &res.1);
        let mut status = res.2;
        // If the response is a JS-challenge / PoW wall and a solver is configured,
        // solve it, inject the cleared cookies, and re-fetch on the fast path.
        if let Some(new_status) = self.try_solve_challenge(&res.0, status, &res.1).await? {
            status = new_status;
        }
        if self.render_mode() {
            self.render_current().await?;
        }
        Ok(
            json!({ "url": res.0, "status": status, "title": title_of(self.tree.as_ref().unwrap()) }),
        )
    }

    // Detect an anti-bot wall on a just-fetched response and, if a solver is set,
    // solve → inject token cookies/headers → re-fetch once. Returns the re-fetch
    // status when it solved, else `None` (no solver / not a challenge / solve
    // failed — the original page stands). Uses the session jar's cookies as the
    // header signal (set-cookie was already ingested into the jar).
    async fn try_solve_challenge(
        &mut self,
        url: &str,
        status: u16,
        body: &str,
    ) -> Result<Option<u16>, String> {
        if self.solver.is_none() {
            return Ok(None);
        }
        let cookie_line = self.jar.cookie_header(url, 0.0);
        let signal: Vec<(String, String)> = cookie_line
            .split("; ")
            .filter(|s| !s.is_empty())
            .map(|c| ("set-cookie".to_string(), c.to_string()))
            .collect();
        let Some(ch) = challenge::detect(url, status, &signal, body) else {
            return Ok(None);
        };
        let ctx = SolveContext {
            user_agent: self.ua.clone().unwrap_or_default(),
            proxy: std::env::var("TURBO_SURF_PROXY")
                .ok()
                .filter(|s| !s.is_empty()),
        };
        // Borrow the solver out so we can mutate the jar/headers while it runs.
        let solver = self.solver.take().unwrap();
        let solved = solver.solve(&ch, &ctx).await;
        self.solver = Some(solver);
        let token = match solved {
            Ok(t) => t,
            Err(_) => return Ok(None), // leave the challenge page in place
        };
        for (k, v) in &token.cookies {
            self.jar
                .set_from_response(url, &[format!("{k}={v}; Path=/")], 0.0);
        }
        for (k, v) in &token.headers {
            self.headers.insert(k.to_ascii_lowercase(), v.clone());
        }
        // Re-fetch with the cleared cookies (same profile/headers/jar).
        let profile = self.profile_for(url);
        let opts = FetchOptions {
            allow_non_html: true,
            headers: self.request_headers(),
            jar: Some(&mut self.jar),
            profile: Some(&profile),
            ..Default::default()
        };
        let res = fetch_html_with(url, opts).await?;
        self.load(&res.0, &res.1);
        Ok(Some(res.2))
    }

    fn render_mode(&self) -> bool {
        matches!(self.mode.as_str(), "fast" | "secure" | "js")
    }

    // Concatenated executable scripts of the current page (inline code + fetched
    // external `src`), in source order — what the render tier runs.
    async fn page_script(&self) -> String {
        let mut inline = Vec::new();
        let mut external = Vec::new();
        if let Some(tree) = &self.tree {
            for &h in tree.query_selector_all("script").iter() {
                match tree.get_attribute(h, "src") {
                    Some(src) => {
                        if let Some(abs) = turbo_surf_core::url::resolve(&self.url, src) {
                            external.push(abs);
                        }
                    }
                    None => inline.push((h, tree.text_content(h))),
                }
            }
        }
        let mut parts = Vec::new();
        for (_, code) in &inline {
            parts.push(code.clone());
        }
        for url in external {
            if let Ok(r) = fetch_html(
                &url,
                FetchOptions {
                    allow_non_html: true,
                    ..Default::default()
                },
            )
            .await
            {
                parts.push(r.html);
            }
        }
        // Join with the render tier's script boundary (not a bare `;`) so each
        // `<script>` body runs as its own top-level program — a throw in one is
        // isolated from the rest, like a browser (see `render` `SCRIPT_BOUNDARY`).
        parts.join(turbo_surf_render::SCRIPT_BOUNDARY)
    }

    // Run the page's own scripts over its DOM (the render tier) and reload the
    // session from the hydrated HTML; appends to the DOM-history trail.
    async fn render_current(&mut self) -> Result<(), String> {
        let html = self.tree.as_ref().map(serialize_doc).unwrap_or_default();
        let script = self.page_script().await;
        let hydrated = turbo_surf_render::render_page(&html, &self.url, &script).await?;
        self.dom_history.push(hydrated.clone());
        self.tree = Some(Tree::parse(&hydrated));
        Ok(())
    }

    async fn goto(&mut self, url: &str) -> Result<Value, String> {
        if !self.url.is_empty() && self.url != "about:blank" {
            self.back.push(self.url.clone());
        }
        self.forward.clear();
        self.fetch_into(url, None, None).await
    }

    async fn reload(&mut self) -> Result<Value, String> {
        let url = self.url.clone();
        self.fetch_into(&url, None, None).await
    }

    async fn go_back(&mut self) -> Result<Value, String> {
        let prev = self.back.pop().ok_or("no back history")?;
        self.forward.push(self.url.clone());
        self.fetch_into(&prev, None, None).await
    }

    async fn go_forward(&mut self) -> Result<Value, String> {
        let next = self.forward.pop().ok_or("no forward history")?;
        self.back.push(self.url.clone());
        self.fetch_into(&next, None, None).await
    }

    // Mutate a control located by selector; returns the new title (or ok).
    fn mutate<F: FnOnce(&mut Tree, Handle)>(
        &mut self,
        selector: &str,
        f: F,
    ) -> Result<Value, String> {
        let tree = self.tree_mut()?;
        let h = tree
            .query_selector(selector)
            .ok_or_else(|| format!("no element matches {selector}"))?;
        f(tree, h);
        Ok(json!({ "ok": true }))
    }

    async fn click(&mut self, selector: &str) -> Result<Value, String> {
        let base = self.url.clone();
        let intent = {
            let tree = self.tree()?;
            let h = tree
                .query_selector(selector)
                .ok_or_else(|| format!("no element matches {selector}"))?;
            view::click_intent(tree, h, &base)
        };
        match intent {
            view::ClickIntent::Navigate(url) => self.goto(&url).await,
            view::ClickIntent::Submit(s) => {
                let method = (s.method != "GET").then_some(s.method);
                self.back.push(base);
                self.forward.clear();
                self.fetch_into(&s.url, method, s.body).await
            }
            view::ClickIntent::Inert => Ok(json!({ "action": "inert" })),
        }
    }

    // Submit a form (selected, else the first <form>) — builds the submission from
    // the form graph and fetches the result.
    async fn submit(&mut self, selector: Option<&str>) -> Result<Value, String> {
        let base = self.url.clone();
        let sub = {
            let tree = self.tree()?;
            let form = match selector {
                Some(s) => tree
                    .query_selector(s)
                    .ok_or_else(|| format!("no element matches {s}"))?,
                None => tree.query_selector("form").ok_or("no form on page")?,
            };
            view::build_submission(tree, form, &base, None)
        };
        let method = (sub.method != "GET").then_some(sub.method);
        self.back.push(base);
        self.forward.clear();
        self.fetch_into(&sub.url, method, sub.body).await
    }

    // Evaluate JS against the current DOM, returning its result (no mutation kept).
    fn eval_js(&self, script: &str) -> Result<Value, String> {
        let html = serialize_doc(self.tree()?);
        turbo_surf_render::run_with_dom(&html, script).map(Value::String)
    }

    // Run JS that mutates the DOM; reload the session from the hydrated result and
    // append to the DOM-history trail.
    async fn inject_js(&mut self, script: &str) -> Result<Value, String> {
        let html = serialize_doc(self.tree()?);
        let base = self.url.clone();
        let hydrated = turbo_surf_render::render_page(&html, &base, script).await?;
        self.dom_history.push(hydrated.clone());
        self.tree = Some(Tree::parse(&hydrated));
        Ok(json!({ "ok": true }))
    }

    // Debug/probe mode: run the current page's own scripts with the fingerprint
    // globals (navigator/screen/chrome/canvas) instrumented, and report what they
    // touched + which reads came back undefined — i.e. what an anti-bot check read
    // and what we still need to shim. Recon, not a render (the page isn't mutated).
    async fn probe(&self) -> Result<Value, String> {
        let html = serialize_doc(self.tree()?);
        let script = self.page_script().await;
        let report = turbo_surf_render::probe_globals(&html, &script)?;
        serde_json::to_value(report).map_err(|e| e.to_string())
    }

    // EXPERIMENTAL: reconstruct an Akamai sensor from the live page. Find the
    // Akamai script, hash it (the key Akamai seeds its shuffle/encryption from),
    // probe what it reads (the shim surface), and build a CANDIDATE sensor_data for
    // every stored SensorVersion seeded by that hash. This is the recon → rebuild
    // loop; candidates still need testing against the live edge (key rotation means
    // a hash-seeded candidate may not be accepted — that's the open question).
    async fn analyze_akamai(&mut self, retry: bool) -> Result<Value, String> {
        use turbo_surf_core::akamai::{generate_sensor_versioned, SensorInput, SensorVersion};
        // Locate the Akamai script: an external <script src> whose body carries the
        // Akamai markers (bmak / sensor_data / _abck).
        let mut script_url = None;
        let mut script_body = String::new();
        if let Some(tree) = &self.tree {
            for &h in tree.query_selector_all("script[src]").iter() {
                let Some(src) = tree.get_attribute(h, "src") else {
                    continue;
                };
                let Some(abs) = turbo_surf_core::url::resolve(&self.url, src) else {
                    continue;
                };
                if let Ok(r) = fetch_html(
                    &abs,
                    FetchOptions {
                        allow_non_html: true,
                        ..Default::default()
                    },
                )
                .await
                {
                    if r.html.contains("bmak") || r.html.contains("sensor_data") {
                        script_url = Some(abs);
                        script_body = r.html;
                        break;
                    }
                }
            }
        }
        if script_body.is_empty() {
            return Err("no Akamai script found on the current page".into());
        }
        let script_hash = format!("{:016x}", fnv_hex(&script_body));
        // What the script reads — the shim surface to satisfy.
        let probe = turbo_surf_render::probe_globals("<html><body></body></html>", &script_body)
            .ok()
            .map(|r| r.shim_needed)
            .unwrap_or_default();
        // A candidate sensor per stored version, seeded by the script hash.
        let input = SensorInput {
            user_agent: self.ua.clone().unwrap_or_default(),
            page_url: self.url.clone(),
            abck: self.jar.cookie_header(&self.url, 0.0),
            bm_sz: String::new(),
            script_hash: script_hash.clone(),
        };
        let built: Vec<(SensorVersion, String)> = SensorVersion::all()
            .iter()
            .map(|&v| (v, generate_sensor_versioned(&input, v)))
            .collect();

        // RETRY MODE: POST each candidate to the sensor endpoint and test whether it
        // clears the wall (the live-acceptance loop). On a hit, the cleared `_abck`
        // is left in the session jar and that candidate is returned as accepted.
        let mut accepted: Option<Value> = None;
        let mut candidates = Vec::new();
        for (v, sensor) in &built {
            let mut entry = json!({ "version": format!("{v:?}"), "sensor_data": sensor });
            if retry && accepted.is_none() {
                let ok = self.test_sensor(sensor).await;
                entry["accepted"] = json!(ok);
                if ok {
                    // Persist the working sensor locally, keyed by script hash +
                    // version, so it can be reused while it stays valid.
                    let saved = save_sensor(&script_hash, &format!("{v:?}"), sensor, &self.url);
                    entry["savedTo"] = json!(saved);
                    accepted = Some(json!({ "version": format!("{v:?}"), "savedTo": saved }));
                }
            }
            candidates.push(entry);
        }
        Ok(json!({
            "scriptUrl": script_url,
            "scriptHash": script_hash,
            "scriptBytes": script_body.len(),
            "shimNeeded": probe,
            "candidates": candidates,
            "retried": retry,
            "accepted": accepted,
            "note": "EXPERIMENTAL — candidates are hash-seeded structural rebuilds. \
                     `retry` POSTs each to the sensor endpoint and tests live \
                     acceptance; key rotation may reject all (none accepted = the \
                     per-version encoding still needs reversing off this script).",
        }))
    }

    // POST a candidate sensor_data to the current page (Akamai's sensor endpoint)
    // and test whether the wall clears: re-fetch with the returned _abck and check
    // the page is no longer a challenge. On success the jar holds the cleared cookie.
    async fn test_sensor(&mut self, sensor: &str) -> bool {
        let url = self.url.clone();
        let body = json!({ "sensor_data": sensor }).to_string();
        // Post the sensor (cookies round-trip through the jar).
        let post = FetchOptions {
            method: Some("POST".into()),
            body: Some(body),
            allow_non_html: true,
            headers: self.request_headers(),
            jar: Some(&mut self.jar),
            ..Default::default()
        };
        if fetch_html_with(&url, post).await.is_err() {
            return false;
        }
        // Re-fetch the page with the (possibly cleared) cookies; accepted if it is
        // no longer detected as an Akamai wall.
        let get = FetchOptions {
            allow_non_html: true,
            headers: self.request_headers(),
            jar: Some(&mut self.jar),
            ..Default::default()
        };
        match fetch_html_with(&url, get).await {
            Ok((u, html, status)) => {
                let cookie = self.jar.cookie_header(&u, 0.0);
                let sig: Vec<(String, String)> = cookie
                    .split("; ")
                    .filter(|s| !s.is_empty())
                    .map(|c| ("set-cookie".to_string(), c.to_string()))
                    .collect();
                status == 200 && challenge::detect(&u, status, &sig, &html).is_none()
            }
            Err(_) => false,
        }
    }

    // Override render-tier navigator fingerprint fields (JSON object merged over
    // the Chrome 149 defaults; every field is individually overridable). Persisted
    // on the session and pushed to the render isolate. `{}` resets to defaults.
    fn set_fingerprint(&mut self, overrides: &Value) -> Result<Value, String> {
        let json = if overrides.is_null() {
            "{}".to_string()
        } else {
            overrides.to_string()
        };
        turbo_surf_render::set_fingerprint(&json);
        self.fingerprint = json.clone();
        Ok(json!({ "ok": true, "fingerprint": overrides.clone() }))
    }

    // Set the session's default layout viewport (px). Zero/absent dimensions are
    // left unchanged. Drives `screenshot` layout when a call omits its own size.
    fn set_viewport(&mut self, args: &Value) -> Result<Value, String> {
        if let Some(w) = arg_u32(args, "width") {
            self.viewport.width = w;
        }
        if let Some(h) = arg_u32(args, "height") {
            self.viewport.height = h;
        }
        Ok(json!({ "viewport": { "width": self.viewport.width, "height": self.viewport.height } }))
    }

    // Render an HTML snapshot into an image (no browser). `snapshot` selects a
    // hydration-trail entry (index into the rendered-DOM history); omitted =
    // the current page. `format` is "png" (default) or "svg". `width`/`height`
    // override the session viewport for this call only. The page's external
    // `<link rel="stylesheet">` sheets are fetched (via the same client — so an
    // impersonated session pulls them with the real fingerprint + cookies) and
    // cascaded, unless `{ external_css: false }`. PNG comes back base64 (MCP
    // transports JSON text), SVG as the document string.
    async fn screenshot(&mut self, args: &Value) -> Result<Value, String> {
        let html = match args.get("snapshot").and_then(Value::as_u64) {
            Some(i) => self.dom_history.get(i as usize).cloned().ok_or_else(|| {
                format!(
                    "screenshot: snapshot {i} out of range (have {})",
                    self.dom_history.len()
                )
            })?,
            None => serialize_doc(self.tree()?),
        };
        let vp = raster::Viewport {
            width: arg_u32(args, "width").unwrap_or(self.viewport.width),
            height: arg_u32(args, "height").unwrap_or(self.viewport.height),
        };
        let want_css = args
            .get("external_css")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let css = if want_css {
            self.fetch_linked_css(&html).await
        } else {
            String::new()
        };
        // `<img>` + `background-image` bytes, fetched over the session client
        // (impersonation + cookies apply), unless `{ images: false }`.
        let want_images = args.get("images").and_then(Value::as_bool).unwrap_or(true);
        let images = if want_images {
            self.fetch_page_images(&html, &css).await
        } else {
            raster::ImageAssets::new()
        };
        // `full_page: true` grows the image to the full content height instead of
        // clipping to the viewport height (width still drives layout).
        let full_page = args
            .get("full_page")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match arg_str(args, "format").unwrap_or("png") {
            "svg" => {
                let svg = raster::screenshot_svg_with_assets(&html, &css, vp, &images, full_page)?;
                Ok(json!({
                    "format": "svg", "mimeType": "image/svg+xml",
                    "width": vp.width, "height": svg_height(&svg).unwrap_or(vp.height), "svg": svg,
                }))
            }
            "png" => {
                let png = raster::screenshot_png_with_assets(&html, &css, vp, &images, full_page)?;
                let (w, h) = png_dims(&png).unwrap_or((vp.width, vp.height));
                Ok(json!({
                    "format": "png", "mimeType": "image/png",
                    "width": w, "height": h, "base64": BASE64.encode(&png),
                }))
            }
            other => Err(format!("screenshot: unknown format '{other}' (png|svg)")),
        }
    }

    // Fetch the page's `<link rel="stylesheet">` sheets and concatenate their
    // bodies in source order, resolving each href against the current page URL
    // via the session client (impersonation + cookies apply). Failures + non-URL
    // hrefs are skipped; the count is capped so a hostile page can't fan out.
    async fn fetch_linked_css(&mut self, html: &str) -> String {
        const MAX_SHEETS: usize = 40;
        let base = self.url.clone();
        let mut css = String::new();
        for href in raster::stylesheet_hrefs(html).into_iter().take(MAX_SHEETS) {
            let Some(url) = turbo_surf_core::url::resolve(&base, &href) else {
                continue;
            };
            if let Ok(body) = self.fetch_body(&url).await {
                css.push_str(&body);
                css.push('\n');
            }
        }
        css
    }

    // Fetch the page's `<img>` + `background-image` bytes, keyed by their raw
    // reference (the resolver key the raster expects), resolving each against the
    // current page URL via the session client. Non-URL / `data:` refs and fetch
    // failures are skipped; the count is capped so a hostile page can't fan out.
    async fn fetch_page_images(&mut self, html: &str, css: &str) -> raster::ImageAssets {
        const MAX_IMAGES: usize = 60;
        let base = self.url.clone();
        let mut assets = raster::ImageAssets::new();
        // `<img>`/inline-style/`<style>`-block refs from the HTML, plus
        // `background-image` refs from the external `<link>` stylesheets — the layout
        // paints those backgrounds, but a HTML-only scan never fetches their bytes.
        let names = raster::image_urls(html)
            .into_iter()
            .chain(raster::image_urls_in_css(css))
            .take(MAX_IMAGES);
        let mut seen = std::collections::HashSet::new();
        for name in names {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(url) = turbo_surf_core::url::resolve(&base, &name) else {
                continue;
            };
            if let Ok(bytes) = self.fetch_image_bytes(&url).await {
                assets.insert(name, bytes);
            }
        }
        assets
    }

    // Fetch a single resource as raw bytes (no charset decode — image data must
    // survive verbatim) over the session client/jar/profile.
    async fn fetch_image_bytes(&mut self, url: &str) -> Result<Vec<u8>, String> {
        self.requests.push(url.to_string());
        let profile = self.profile_for(url);
        let opts = FetchOptions {
            allow_non_html: true,
            headers: self.request_headers(),
            jar: Some(&mut self.jar),
            profile: Some(&profile),
            ..Default::default()
        };
        Ok(fetch_bytes(url, opts).await.map_err(|e| e.to_string())?.1)
    }

    // Report the active stealth posture: the per-host fingerprint profile this
    // session would send, whether a challenge solver is wired, and the pool size.
    fn stealth_status(&self) -> Value {
        let key = if self.url.is_empty() {
            "about:blank"
        } else {
            &self.url
        };
        let p = self.profile_for(key);
        json!({
            "profile": {
                "userAgent": p.user_agent,
                "platform": p.nav_platform,
                "chromeMajor": p.chrome_major,
                "acceptLanguage": p.accept_language,
            },
            "solver": self.solver.as_ref().map(|s| s.name()),
            "poolSize": fingerprint::pool_size(),
            "renderFingerprintOverrides": if self.fingerprint.is_empty() {
                json!({})
            } else {
                serde_json::from_str(&self.fingerprint).unwrap_or(json!({}))
            },
        })
    }

    async fn fetch_body(&mut self, url: &str) -> Result<String, String> {
        self.requests.push(url.to_string());
        let profile = self.profile_for(url);
        let opts = FetchOptions {
            allow_non_html: true,
            headers: self.request_headers(),
            jar: Some(&mut self.jar),
            profile: Some(&profile),
            ..Default::default()
        };
        Ok(fetch_html_with(url, opts).await?.1)
    }
}

// Fetch returning (final_url, html, status) — small adapter over net.
async fn fetch_html_with(
    url: &str,
    opts: FetchOptions<'_>,
) -> Result<(String, String, u16), String> {
    let res = fetch_html(url, opts).await.map_err(|e| e.to_string())?;
    Ok((res.final_url, res.html, res.status))
}

// The (width, height) in a PNG's IHDR (big-endian at bytes 16..24), so the
// screenshot reply reports the real image size — important for `full_page`,
// where the height is the content height, not the viewport.
fn png_dims(png: &[u8]) -> Option<(u32, u32)> {
    let b = png.get(16..24)?;
    let w = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let h = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
    Some((w, h))
}

// The `height="N"` of the raster's SVG `<svg>` header (the raster emits an
// integer px height), for the same `full_page`-aware size reporting as PNG.
fn svg_height(svg: &str) -> Option<u32> {
    let at = svg.find("height=\"")? + "height=\"".len();
    let rest = &svg[at..];
    let end = rest.find('"')?;
    rest[..end].trim().parse().ok()
}

// FNV-1a (64-bit) over a string — the Akamai script-hash seed for analyze_akamai.
fn fnv_hex(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// Persist a working Akamai sensor locally so it can be reused while valid. Dir is
// `TURBO_SURF_SENSOR_DIR` (default `./akamai-sensors`); file is keyed by script
// hash + version. Returns the path written, or None on failure (best-effort).
fn save_sensor(script_hash: &str, version: &str, sensor: &str, page_url: &str) -> Option<String> {
    let dir = std::env::var("TURBO_SURF_SENSOR_DIR").unwrap_or_else(|_| "akamai-sensors".into());
    std::fs::create_dir_all(&dir).ok()?;
    let path = format!("{dir}/{script_hash}-{version}.json");
    let blob = json!({
        "scriptHash": script_hash,
        "version": version,
        "pageUrl": page_url,
        "sensor_data": sensor,
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&blob).ok()?).ok()?;
    Some(path)
}

fn title_of(tree: &Tree) -> String {
    tree.query_selector("title")
        .map(|h| tree.text_content(h).trim().to_string())
        .unwrap_or_default()
}

fn serialize_doc(tree: &Tree) -> String {
    serialize_inner(tree, tree.root())
}

// --- tool registry ----------------------------------------------------------

/// `tools/list` descriptors (name + one-line description + minimal input schema).
// A compact Playwright-shaped API defined over the render isolate's live `document`
// (rtdom). Backs the `run_playwright` tool — a script using `page`/`locator`/`getBy*`/
// `expect` runs against the engine, no browser. `console.*` is captured into __LOGS;
// `test(...)` blocks are collected and run by the wrapper. goto inside a script does a
// best-effort no-JS re-fetch+reparse (load the initial page via the tool's `url`/mode
// for SPA hydration).
const PLAYWRIGHT_PRELUDE: &str = r###"
(function(){
  globalThis.__LOGS = [];
  var cap = function(){ try { globalThis.__LOGS.push(Array.prototype.map.call(arguments, String).join(' ')); } catch(e){} };
  globalThis.console = { log: cap, info: cap, warn: cap, error: cap, debug: function(){} };
  var TID = globalThis.__TESTID_ATTR || 'data-testid';
  var norm = function(s){ return String(s==null?'':s).replace(/ /g,' ').replace(/\s+/g,' ').trim(); };
  function cssq(v){ return '"' + String(v).replace(/"/g,'\\"') + '"'; }
  function mk(getEls){
    return {
      _get: getEls,
      first: function(){ return mk(function(){ var e=getEls(); return e.length?[e[0]]:[]; }); },
      last: function(){ return mk(function(){ var e=getEls(); return e.length?[e[e.length-1]]:[]; }); },
      nth: function(i){ return mk(function(){ var e=getEls(); return e[i]?[e[i]]:[]; }); },
      locator: function(s){ return mk(function(){ var out=[]; getEls().forEach(function(el){ Array.prototype.push.apply(out, Array.prototype.slice.call(el.querySelectorAll(s))); }); return out; }); },
      getByTestId: function(id){ return this.locator('['+TID+'='+cssq(id)+']'); },
      count: function(){ return Promise.resolve(getEls().length); },
      _one: function(){ var e=getEls(); if(!e.length) throw new Error('locator matched no elements'); return e[0]; },
      textContent: function(){ var e=getEls(); return Promise.resolve(e.length? e[0].textContent : null); },
      innerText: function(){ var e=getEls(); return Promise.resolve(e.length? norm(e[0].textContent) : ''); },
      getAttribute: function(n){ var e=getEls(); return Promise.resolve(e.length? e[0].getAttribute(n) : null); },
      inputValue: function(){ var e=getEls(); return Promise.resolve(e.length? (e[0].value!=null?e[0].value:'') : ''); },
      isVisible: function(){ return Promise.resolve(getEls().length>0); },
      isChecked: function(){ var e=getEls(); return Promise.resolve(e.length? !!e[0].checked : false); },
      fill: function(v){ this._one().value = v; return Promise.resolve(); },
      type: function(v){ this._one().value = v; return Promise.resolve(); },
      check: function(){ this._one().checked = true; return Promise.resolve(); },
      uncheck: function(){ this._one().checked = false; return Promise.resolve(); },
      click: function(){ var el=this._one(); if (el.click) el.click(); return Promise.resolve(); },
    };
  }
  var byCss = function(s){ return mk(function(){ return Array.prototype.slice.call(document.querySelectorAll(s)); }); };
  var byPred = function(pred){ return mk(function(){ return Array.prototype.slice.call(document.querySelectorAll('*')).filter(pred); }); };
  globalThis.page = {
    goto: function(u){ return fetch(u).then(function(r){ return r.text(); }).then(function(b){
        try { var m=/<body[^>]*>([\s\S]*?)<\/body>/i.exec(b); if (document.body) document.body.innerHTML = m? m[1] : b; } catch(e){}
        return { status: function(){ return 200; }, ok: function(){ return true; }, url: function(){ return u; } };
      }); },
    locator: byCss,
    getByTestId: function(id){ return byCss('['+TID+'='+cssq(id)+']'); },
    getByRole: function(r, o){ var name = o && o.name; return byPred(function(el){ var role=el.getAttribute('role')||IMPLICIT_ROLE(el); if (role!==r) return false; if (name==null) return true; return norm(el.textContent).indexOf(norm(name))>=0 || (el.getAttribute('aria-label')||'').indexOf(name)>=0; }); },
    getByText: function(t){ return byPred(function(el){ return norm(el.textContent).indexOf(norm(t))>=0; }); },
    getByLabel: function(t){ return byCss('[aria-label='+cssq(t)+']'); },
    getByPlaceholder: function(t){ return byCss('[placeholder*='+cssq(t)+']'); },
    title: function(){ var e=document.querySelector('title'); return Promise.resolve(e? e.textContent : ''); },
    content: function(){ return Promise.resolve(document.documentElement ? document.documentElement.outerHTML : ''); },
    innerText: function(s){ return byCss(s).innerText(); },
    url: function(){ return globalThis.location ? globalThis.location.href : ''; },
    fill: function(s,v){ return byCss(s).fill(v); },
    click: function(s){ return byCss(s).click(); },
    check: function(s){ return byCss(s).check(); },
    waitForTimeout: function(){ return Promise.resolve(); },
    waitForLoadState: function(){ return Promise.resolve(); },
    waitForURL: function(){ return Promise.resolve(); },
    waitForSelector: function(s){ return Promise.resolve(byCss(s)); },
  };
  function IMPLICIT_ROLE(el){ var t=(el.tagName||'').toLowerCase(); return ({a:'link',button:'button',h1:'heading',h2:'heading',h3:'heading',nav:'navigation',input:'textbox',select:'combobox'})[t] || ''; }
  function assert(pass, msg){ if(!pass) throw new Error(msg); }
  globalThis.expect = function(v){
    var make = function(neg){ return {
      get not(){ return make(!neg); },
      toBeVisible: function(){ return v.count().then(function(c){ assert((c>0)!==neg, 'expected element to be visible'); }); },
      toBeHidden: function(){ return v.count().then(function(c){ assert((c===0)!==neg, 'expected element to be hidden'); }); },
      toHaveCount: function(n){ return v.count().then(function(c){ assert((c===n)!==neg, 'expected count '+n+', got '+c); }); },
      toHaveText: function(s){ return v.textContent().then(function(t){ t=norm(t); var p=(s instanceof RegExp)?s.test(t):(t===norm(s)); assert(p!==neg, 'expected text '+s+', got "'+t+'"'); }); },
      toContainText: function(s){ return v.textContent().then(function(t){ t=norm(t); var p=(s instanceof RegExp)?s.test(t):(t.indexOf(norm(s))>=0); assert(p!==neg, 'expected text to contain '+s+', got "'+t+'"'); }); },
      toHaveValue: function(s){ return v.inputValue().then(function(got){ var p=(s instanceof RegExp)?s.test(got):(got===s); assert(p!==neg, 'expected value '+s+', got "'+got+'"'); }); },
      toHaveAttribute: function(n, val){ return v.getAttribute(n).then(function(got){ var p=(val===undefined)?got!==null:got===val; assert(p!==neg, 'expected attribute '+n+'='+val+', got '+got); }); },
      toBeChecked: function(){ return v.isChecked().then(function(c){ assert(c!==neg, 'expected element to be checked'); }); },
      toBe: function(x){ assert((v===x)!==neg, 'expected '+x+', got '+v); },
      toEqual: function(x){ assert((JSON.stringify(v)===JSON.stringify(x))!==neg, 'expected equal'); },
      toContain: function(x){ assert(((typeof v==='string'? v.indexOf(x)>=0 : (Array.isArray(v)&&v.indexOf(x)>=0)))!==neg, 'expected to contain '+x); },
      toBeTruthy: function(){ assert((!!v)!==neg, 'expected truthy'); },
      toBeFalsy: function(){ assert((!v)!==neg, 'expected falsy'); },
      toBeNull: function(){ assert((v===null)!==neg, 'expected null'); },
      toBeGreaterThan: function(n){ assert((v>n)!==neg, 'expected > '+n); },
      toBeLessThan: function(n){ assert((v<n)!==neg, 'expected < '+n); },
    }; };
    return make(false);
  };
  globalThis.__TESTS = [];
  globalThis.test = function(name, fn){ globalThis.__TESTS.push({ name: name, fn: fn }); };
  globalThis.test.describe = function(n, fn){ if (fn) fn(); };
  globalThis.test.skip = function(){};
  globalThis.test.beforeEach = function(){}; globalThis.test.afterEach = function(){};
  globalThis.test.beforeAll = function(){}; globalThis.test.afterAll = function(){};
})();
"###;

async fn tool_run_playwright(session: &mut Session, args: &Value) -> Result<Value, String> {
    let script = arg_str(args, "script").ok_or("run_playwright: missing 'script'")?;
    let test_id = arg_str(args, "testIdAttribute").unwrap_or("data-testid");
    if let Some(url) = arg_str(args, "url") {
        session.goto(url).await?; // honors session.mode (hydrates SPA when a JS mode is set)
    }
    let (html, base) = {
        let tree = session.tree()?;
        let html = serialize_inner(tree, tree.root());
        let base = if session.url.is_empty() {
            "about:blank".to_string()
        } else {
            session.url.clone()
        };
        (html, base)
    };
    // Frame: config + prelude + the user's script (+ run any test() blocks) → __RESULT.
    let program = format!(
        "globalThis.__TESTID_ATTR={};\n{}\nglobalThis.__RESULT='';(async function(){{ try {{\n{}\n; if (globalThis.__TESTS && globalThis.__TESTS.length) {{ for (var i=0;i<globalThis.__TESTS.length;i++) {{ await globalThis.__TESTS[i].fn({{ page: globalThis.page, expect: globalThis.expect }}); }} }} globalThis.__RESULT = JSON.stringify({{ ok:true, ran:(globalThis.__TESTS||[]).map(function(t){{return t.name;}}), logs:globalThis.__LOGS }}); }} catch (e) {{ globalThis.__RESULT = JSON.stringify({{ ok:false, error:String((e&&e.stack)||e), logs:globalThis.__LOGS }}); }} }})();",
        serde_json::to_string(test_id).unwrap_or_else(|_| "\"data-testid\"".into()),
        PLAYWRIGHT_PRELUDE,
        script,
    );
    let out = turbo_surf_render::eval_async(&html, &base, &program).await?;
    serde_json::from_str(&out).map_err(|e| format!("run_playwright: bad result ({e}); raw={out}"))
}

// One-shot `goto` + `markdown`: fetch a URL and return its rendered markdown in a
// single call, so a caller can treat surf as stateless instead of driving a
// `goto` then `markdown` pair over a long-lived session. It composes the same
// internals those two tools use — no new behaviour. `mode` (opt-in) selects the JS
// render tier for this fetch; `links:true` also returns the page's absolute links.
async fn tool_fetch_markdown(session: &mut Session, args: &Value) -> Result<Value, String> {
    let url = arg_str(args, "url").ok_or("fetch_markdown: missing 'url'")?;
    // Opt-in only: leave the session's current tier untouched when unset.
    if let Some(mode) = arg_str(args, "mode") {
        session.mode = mode.to_string();
    }
    // `goto` already yields { url, status, title }; extend it with the content.
    let mut out = session.goto(url).await?;
    let tree = session.tree()?;
    let base = session.url.clone();
    out["markdown"] = json!(view::markdown(tree, tree.root(), &base));
    if args.get("links").and_then(Value::as_bool).unwrap_or(false) {
        out["links"] = json!(view::links(tree, &base));
    }
    Ok(out)
}

// Concurrent, order-preserving, no-JS sibling of `fetch_markdown`: fetch + parse
// + render markdown for each URL. A per-URL failure lands as `{url,error}` and
// never aborts the batch (mirroring `turbo_surf_page::batch`'s per-URL `Err`).
// It runs its own bounded loop rather than routing through `page::batch` — that
// path returns `crawl::Nav`, which discards the parse tree we render markdown off.
async fn tool_fetch_markdown_batch(args: &Value) -> Result<Value, String> {
    use futures_util::stream::{self, StreamExt};
    let urls: Vec<String> = args
        .get("urls")
        .and_then(Value::as_array)
        .ok_or("fetch_markdown_batch: missing 'urls' array")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let concurrency = args.get("concurrency").and_then(Value::as_u64).unwrap_or(4) as usize;
    let want_links = args.get("links").and_then(Value::as_bool).unwrap_or(false);
    let out: Vec<Value> = stream::iter(urls)
        .map(|url| async move {
            match fetch_markdown_entry(&url, want_links).await {
                Ok(entry) => entry,
                Err(e) => json!({ "url": url, "error": e }),
            }
        })
        .buffered(concurrency.max(1))
        .collect()
        .await;
    Ok(json!(out))
}

// Fetch + parse + render markdown for one URL (the batch's per-URL unit). The
// tree is kept locally so markdown/links render off the same parse — hence the
// self-contained fetch instead of `page::batch`, whose `Nav` drops the tree.
async fn fetch_markdown_entry(url: &str, want_links: bool) -> Result<Value, String> {
    let opts = FetchOptions {
        allow_non_html: true,
        ..Default::default()
    };
    let res = fetch_html(url, opts).await.map_err(|e| e.to_string())?;
    let tree = Tree::parse(&res.html);
    let base = res.final_url;
    let mut entry = json!({
        "url": base,
        "status": res.status,
        "title": title_of(&tree),
        "markdown": view::markdown(&tree, tree.root(), &base),
    });
    if want_links {
        entry["links"] = json!(view::links(&tree, &base));
    }
    Ok(entry)
}

// --- web search (SERP scrape) -----------------------------------------------

// A declarative parse strategy: everything the interpreter needs to build a SERP
// query URL and pull organic results out of the response — all DATA, no engine
// selector lives in compiled code. Loaded from the bundled `search-strategies.json`,
// a user strategies dir, or `web_search_load_strategy` at runtime (see the registry).
#[derive(Clone, Debug, serde::Deserialize)]
struct Strategy {
    /// The id callers pass as `engine`.
    engine: String,
    /// The dated markup snapshot this strategy targets (informational).
    #[serde(default)]
    version: String,
    /// `{query}` (%-encoded), `{limit}`, `{base}` are interpolated into this.
    query_url: String,
    /// "html" (selectors) | "json" (`json_path`). Defaults to html.
    #[serde(default = "default_html_format")]
    format: String,
    /// Fetch tier: "no-js" | "fast" | "secure". Empty == no-js. fast/secure render
    /// the SERP's own JS on a throwaway session before parsing.
    #[serde(default)]
    mode: String,
    // --- html strategies: selectors, `title`/`link`/`snippet` scoped to `result_container` ---
    #[serde(default)]
    result_container: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    link: String,
    /// Optional: unwrap `/url?<param>=<real>` or `//host/l/?<param>=<real>` redirect
    /// wrappers (Google `q`, DuckDuckGo `uddg`).
    #[serde(default)]
    link_redirect_param: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
    // --- json strategies (searxng, …): dotted paths into the response ---
    #[serde(default)]
    json_path: Option<JsonPath>,
}

// The response shape for a `format:"json"` engine: `results` is the (dot-pathed)
// array of hits; `title`/`url`/`snippet` are dot-paths within each hit.
#[derive(Clone, Debug, serde::Deserialize)]
struct JsonPath {
    results: String,
    title: String,
    url: String,
    #[serde(default)]
    snippet: Option<String>,
}

fn default_html_format() -> String {
    "html".to_string()
}

impl Strategy {
    // Load-time validation for `web_search_load_strategy`: engine id + a `{query}` slot
    // are always required; the rest depends on `format`.
    fn validate(&self) -> Result<(), String> {
        if self.engine.trim().is_empty() {
            return Err("web_search_load_strategy: 'engine' must be non-empty".into());
        }
        if !self.query_url.contains("{query}") {
            return Err("web_search_load_strategy: 'query_url' must contain '{query}'".into());
        }
        match self.format.as_str() {
            "json" => {
                let jp = self
                    .json_path
                    .as_ref()
                    .ok_or("web_search_load_strategy: format 'json' requires 'json_path'")?;
                if jp.results.is_empty() || jp.title.is_empty() || jp.url.is_empty() {
                    return Err(
                        "web_search_load_strategy: 'json_path' needs results/title/url".into(),
                    );
                }
            }
            "html" => {
                if self.result_container.is_empty() || self.title.is_empty() || self.link.is_empty()
                {
                    return Err("web_search_load_strategy: format 'html' requires \
                                result_container/title/link"
                        .into());
                }
            }
            // Classname-free structural extraction (google): no selectors to validate.
            "structural" => {}
            other => {
                return Err(format!(
                    "web_search_load_strategy: unknown format '{other}' (html|json|structural)"
                ))
            }
        }
        Ok(())
    }
}

// One organic search hit. `snippet` is omitted from the JSON when the markup
// carried none, so the reply stays `[{title,url,snippet?}]`.
#[derive(Debug)]
struct SearchResult {
    title: String,
    url: String,
    snippet: Option<String>,
}

impl SearchResult {
    fn to_json(&self) -> Value {
        let mut o = json!({ "title": self.title, "url": self.url });
        if let Some(s) = &self.snippet {
            o["snippet"] = json!(s);
        }
        o
    }
}

// --- the interpreter (pure; no network) -------------------------------------

// Percent-encode a query string for a URL query value (spaces → `+`).
fn encode_query(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

// Collapse runs of whitespace to single spaces + trim — SERP markup is padded
// with newlines/indentation we don't want in titles/snippets.
fn norm_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// Build the fetch URL from a strategy's `query_url` template. `{query}` is
// %-encoded, `{limit}` is the count, `{base}` is the (trailing-slash-trimmed)
// instance root — an error if the template needs `{base}` but none was given.
fn build_query_url(
    strategy: &Strategy,
    query: &str,
    limit: usize,
    base: Option<&str>,
) -> Result<String, String> {
    let mut url = strategy.query_url.clone();
    if url.contains("{base}") {
        let base = base
            .ok_or_else(|| format!("web_search: engine '{}' requires 'base'", strategy.engine))?;
        url = url.replace("{base}", base.trim_end_matches('/'));
    }
    url = url.replace("{query}", &encode_query(query));
    url = url.replace("{limit}", &limit.to_string());
    Ok(url)
}

// PURE result parser: a fetched SERP body → organic results, rank order, capped at
// `limit`. Kept free of the network so it's unit-testable off saved fixtures.
fn parse_serp(strategy: &Strategy, body: &str, limit: usize) -> Vec<SearchResult> {
    match strategy.format.as_str() {
        "json" => parse_json_serp(strategy, body, limit),
        "structural" => parse_structural_serp(body, limit),
        _ => parse_html_serp(strategy, body, limit),
    }
}

/// True when `url`'s host is a search-engine-internal / asset host (google's own
/// nav, gstatic, googleusercontent, google account/policy links) — never an organic
/// result, so it's filtered out of the structural pass.
fn is_serp_internal_host(url: &str) -> bool {
    let host = turbo_surf_core::url::host_of(url).unwrap_or_default();
    host.ends_with("google.com")
        || host.ends_with("gstatic.com")
        || host.ends_with("googleusercontent.com")
        || host.ends_with("google.co")
        || host.contains(".google.")
}

/// Classname-free organic-result extraction — for engines (google) whose result
/// container/title classes are obfuscated and rotate frequently, making a selector
/// strategy brittle. The stable STRUCTURE doesn't change: an organic result is an
/// `<a href="http…">` that CONTAINS an `<h3>` (its title) and points off-site. We
/// walk those anchors, take the h3 text as the title and the anchor href as the URL,
/// skip engine-internal hosts, and dedup by URL.
fn parse_structural_serp(html: &str, limit: usize) -> Vec<SearchResult> {
    let tree = Tree::parse(html);
    let mut out: Vec<SearchResult> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for &a in tree.query_selector_all("a[href]").iter() {
        if out.len() >= limit {
            break;
        }
        let href = tree.get_attribute(a, "href").unwrap_or_default();
        let url = href.trim();
        if !url.starts_with("http") || is_serp_internal_host(url) {
            continue;
        }
        // The title-carrying <h3> must be INSIDE this anchor (the organic-result shape).
        let Some(h3) = find_desc(&tree, a, "h3") else {
            continue;
        };
        let title = norm_ws(&tree.text_content(h3));
        if title.is_empty() || !seen.insert(url.to_string()) {
            continue;
        }
        out.push(SearchResult {
            title,
            url: url.to_string(),
            snippet: None,
        });
    }
    out
}

// First descendant of `container` matching `selector` (scoped: we only walk the
// container's subtree, so `matches`' ancestor-aware combinators stay in-container).
fn find_desc(tree: &Tree, container: Handle, selector: &str) -> Option<Handle> {
    tree.descendants(container)
        .into_iter()
        .find(|&h| tree.matches(h, selector))
}

// Unwrap an engine redirect href to the real destination when the strategy names a
// wrapper param: DuckDuckGo's `//duckduckgo.com/l/?uddg=<enc-url>` and Google's
// `/url?q=<enc-url>` carry the target in that query param. No param (or the param
// absent from the href) → the href is already the real URL.
fn decode_redirect(param: Option<&str>, href: &str) -> Option<String> {
    let Some(key) = param else {
        return Some(href.to_string());
    };
    // Re-absolutize so `url::Url` will parse it: protocol-relative (`//host/..`)
    // and path-only (`/url?..`) wrappers both need a scheme+host to parse.
    let abs = if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://redirect.invalid{href}")
    } else {
        href.to_string()
    };
    let parsed = url::Url::parse(&abs).ok()?;
    let unwrapped = parsed
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned());
    Some(unwrapped.unwrap_or_else(|| href.to_string()))
}

// Scoped selector extraction: one hit per `result_container`, with `title`/`link`/
// `snippet` matched inside it and `link_redirect_param` decoded. `title` falls back
// to the link anchor when its selector misses (DDG/Bing: the anchor *is* the title).
fn parse_html_serp(strategy: &Strategy, html: &str, limit: usize) -> Vec<SearchResult> {
    let tree = Tree::parse(html);
    let mut out = Vec::new();
    for &container in tree.query_selector_all(&strategy.result_container).iter() {
        if out.len() >= limit {
            break;
        }
        let Some(link_h) = find_desc(&tree, container, &strategy.link) else {
            continue;
        };
        let href = tree.get_attribute(link_h, "href").unwrap_or_default();
        // Skip entries whose url is empty / unparseable / not http(s).
        let url = match decode_redirect(strategy.link_redirect_param.as_deref(), href.trim())
            .filter(|u| u.starts_with("http"))
        {
            Some(u) => u,
            None => continue,
        };
        let title_h = find_desc(&tree, container, &strategy.title).unwrap_or(link_h);
        let title = norm_ws(&tree.text_content(title_h));
        let snippet = strategy
            .snippet
            .as_deref()
            .and_then(|s| find_desc(&tree, container, s))
            .map(|h| norm_ws(&tree.text_content(h)))
            .filter(|s| !s.is_empty());
        out.push(SearchResult {
            title,
            url,
            snippet,
        });
    }
    out
}

// Walk a dot-separated path (`a.b.c`) into a JSON value.
fn json_lookup<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

// JSON-API engines (searxng, …): walk `json_path` — `results[]` of
// `{title,url,snippet}` by dotted path. Entries missing a url are skipped.
fn parse_json_serp(strategy: &Strategy, body: &str, limit: usize) -> Vec<SearchResult> {
    let Some(jp) = &strategy.json_path else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(arr) = json_lookup(&v, &jp.results).and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|r| {
            let url = json_lookup(r, &jp.url)
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())?;
            let title = json_lookup(r, &jp.title)
                .and_then(Value::as_str)
                .unwrap_or("");
            let snippet = jp
                .snippet
                .as_deref()
                .and_then(|s| json_lookup(r, s))
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.is_empty());
            Some(SearchResult {
                title: title.to_string(),
                url: url.to_string(),
                snippet,
            })
        })
        .take(limit)
        .collect()
}

// --- the layered registry ----------------------------------------------------

// The bundled default strategies (duckduckgo/bing/google/searxng/baidu), embedded
// at compile time. Still DATA — parsed at load into the registry, never a Rust
// table — so a release that only edits this JSON updates every engine.
const BUNDLED_STRATEGIES_JSON: &str = include_str!("search-strategies.json");

// The two process-wide registry layers, loaded once: the bundled defaults and any
// user-dir strategies (from `TURBO_SURF_SEARCH_STRATEGIES`). The session override
// map is the third, highest-precedence layer and lives on `Session`.
struct StaticRegistry {
    built_in: HashMap<String, Strategy>,
    user_dir: HashMap<String, Strategy>,
}

// Parse a JSON array of strategy objects into an engine-keyed map (a malformed
// document yields an empty map; a gate test asserts the bundled default parses).
fn strategies_from_json_array(json: &str) -> HashMap<String, Strategy> {
    serde_json::from_str::<Vec<Strategy>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.engine.clone(), s))
        .collect()
}

// Load user-supplied strategies from `TURBO_SURF_SEARCH_STRATEGIES` (a dir of
// `*.json`, one strategy per file). Non-fatal: an unset/absent dir yields an empty
// map, and a bad file is logged and skipped (it never blocks startup).
fn load_user_dir() -> HashMap<String, Strategy> {
    let mut map = HashMap::new();
    let Ok(dir) = std::env::var("TURBO_SURF_SEARCH_STRATEGIES") else {
        return map;
    };
    if dir.is_empty() {
        return map;
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Strategy>(&text) {
                Ok(s) if !s.engine.trim().is_empty() => {
                    map.insert(s.engine.clone(), s);
                }
                Ok(_) => eprintln!("search: skipping {} (empty engine id)", path.display()),
                Err(e) => eprintln!("search: skipping {} ({e})", path.display()),
            },
            Err(e) => eprintln!("search: cannot read {} ({e})", path.display()),
        }
    }
    map
}

fn static_registry() -> &'static StaticRegistry {
    static REG: OnceLock<StaticRegistry> = OnceLock::new();
    REG.get_or_init(|| StaticRegistry {
        built_in: strategies_from_json_array(BUNDLED_STRATEGIES_JSON),
        user_dir: load_user_dir(),
    })
}

// Resolve an engine id to its active strategy + source, precedence high→low:
// session override → user dir → bundled default.
fn resolve_strategy(session: &Session, engine: &str) -> Result<(Strategy, &'static str), String> {
    if let Some(s) = session.search_overrides.get(engine) {
        return Ok((s.clone(), "custom"));
    }
    let reg = static_registry();
    if let Some(s) = reg.user_dir.get(engine) {
        return Ok((s.clone(), "user-dir"));
    }
    if let Some(s) = reg.built_in.get(engine) {
        return Ok((s.clone(), "built-in"));
    }
    Err(format!(
        "unknown engine '{engine}' (load one with web_search_load_strategy)"
    ))
}

// List every active strategy with its winning source, sorted by engine id. Layers
// are inserted low→high precedence so the higher layer overwrites the entry.
fn list_active_strategies(session: &Session) -> Vec<Value> {
    let reg = static_registry();
    let mut seen: BTreeMap<String, (String, &'static str, String)> = BTreeMap::new();
    let mut add = |engine: &str, s: &Strategy, source: &'static str| {
        seen.insert(
            engine.to_string(),
            (s.version.clone(), source, s.format.clone()),
        );
    };
    for (e, s) in &reg.built_in {
        add(e, s, "built-in");
    }
    for (e, s) in &reg.user_dir {
        add(e, s, "user-dir");
    }
    for (e, s) in &session.search_overrides {
        add(e, s, "custom");
    }
    seen.into_iter()
        .map(|(engine, (version, source, format))| {
            json!({ "engine": engine, "version": version, "source": source, "format": format })
        })
        .collect()
}

// --- the search tools --------------------------------------------------------

// Fetch a SERP for a strategy. STATELESS re: the caller's session — it uses a fresh
// per-host Chrome identity + consent-cookie seeding, and for a render-tier strategy
// (mode fast/secure) runs the page's own JS on a THROWAWAY session, so the caller's
// current page/jar are never touched.
async fn fetch_serp(strategy: &Strategy, url: &str, force_browser: bool) -> Result<String, String> {
    // Browser mode: hand the SERP URL to a real-browser sidecar (opt-in, chromium
    // stays OUT of the engine) and parse the rendered results HTML it returns. This
    // is the path for engines gated behind a browser-integrity wall (google's
    // BotGuard/enablejs), which no headless fetch/JS-tier clears — a real browser does.
    if force_browser || strategy.mode == "browser" {
        return browser_fetch_serp(url).await;
    }
    let profile = fingerprint::select(&turbo_surf_core::url::host_of(url).unwrap_or_default());
    let opts = FetchOptions {
        allow_non_html: true, // json engines return non-HTML bodies
        profile: Some(&profile),
        bypass_consent: true,
        ..Default::default()
    };
    let res = fetch_html(url, opts).await.map_err(|e| e.to_string())?;
    if matches!(strategy.mode.as_str(), "fast" | "secure" | "js") {
        let mut tmp = Session::new();
        tmp.load(&res.final_url, &res.html);
        tmp.render_current().await?;
        return Ok(serialize_doc(tmp.tree()?));
    }
    Ok(res.html)
}

/// Fetch a SERP through the opt-in browser sidecar named by the
/// `TURBO_SURF_BROWSER_FETCH_CMD` env var (e.g. `node harness/browser-solver/fetch-serp.mjs`).
/// Contract: we write `{"url":"…"}\n` to its stdin; it navigates a real browser and
/// writes `{"html":"…","finalUrl":"…","status":200}` to stdout. Chromium is never
/// linked into the engine — this shells out to a dev/deploy-provided browser.
async fn browser_fetch_serp(url: &str) -> Result<String, String> {
    let cmd = std::env::var("TURBO_SURF_BROWSER_FETCH_CMD").map_err(|_| {
        "web_search browser mode needs the TURBO_SURF_BROWSER_FETCH_CMD env var (a \
         real-browser sidecar; run web_search_setup_browser, or `node scripts/browser-sidecar/fetch-serp.mjs`)"
            .to_string()
    })?;
    browser_fetch_serp_cmd(url, &cmd).await
}

/// Run a specific browser-sidecar command for `url` (the env-independent core, so the
/// contract is unit-testable with a stub command). See [`browser_fetch_serp`].
async fn browser_fetch_serp_cmd(url: &str, cmd: &str) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;
    let mut parts = cmd.split_whitespace();
    let prog = parts
        .next()
        .ok_or("TURBO_SURF_BROWSER_FETCH_CMD is empty")?;
    let mut child = tokio::process::Command::new(prog)
        .args(parts)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn browser sidecar ({prog}): {e}"))?;
    let req = json!({ "url": url }).to_string();
    child
        .stdin
        .take()
        .ok_or("no sidecar stdin")?
        .write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "browser sidecar failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stdout)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    let v: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("browser sidecar returned non-JSON: {e}"))?;
    // The sidecar flags an abuse wall (google /sorry captcha) — surface it as a clear
    // error instead of parsing a captcha page into zero silent results.
    if v.get("blocked").and_then(Value::as_bool).unwrap_or(false) {
        return Err(format!(
            "search engine served an anti-abuse captcha (blocked); the exit IP is likely \
             rate-limited — {}",
            v.get("finalUrl").and_then(Value::as_str).unwrap_or("")
        ));
    }
    v.get("html")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "browser sidecar response has no 'html'".to_string())
}

// `web_search`: a web search via a one-shot SERP scrape, driven entirely by the active
// parse Strategy (bundled / user-dir / session override — see `resolve_strategy`).
// STATELESS: it reads the session's override map but never touches the current page,
// cookie jar, or history — a caller's page survives a search. Returns organic
// results as `[{title,url,snippet?}]`. Engine precedence: explicit `engine` arg →
// session default (`web_search_set_engine`) → `"duckduckgo"`.
async fn tool_search(session: &Session, args: &Value) -> Result<Value, String> {
    let query = arg_str(args, "query").ok_or("web_search: missing 'query'")?;
    let engine = pick_search_engine(session, args);
    let (strategy, _source) = resolve_strategy(session, engine)?;
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
    let url = build_query_url(&strategy, query, limit, arg_str(args, "base"))?;
    // `browser:true` forces the real-browser sidecar for this call (for engines gated
    // behind a browser-integrity wall like google); else the strategy's own mode drives.
    let force_browser = args
        .get("browser")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let body = fetch_serp(&strategy, &url, force_browser).await?;
    let results = parse_serp(&strategy, &body, limit);
    Ok(json!(results
        .iter()
        .map(SearchResult::to_json)
        .collect::<Vec<_>>()))
}

// Engine precedence for `web_search`: explicit `engine` arg → session default (set via
// `web_search_set_engine`) → the `"duckduckgo"` fallback.
fn pick_search_engine<'a>(session: &'a Session, args: &'a Value) -> &'a str {
    arg_str(args, "engine")
        .or(session.default_search_engine.as_deref())
        .unwrap_or("duckduckgo")
}

// `web_search_strategies`: list every active strategy → `[{engine,version,source,format}]`.
fn tool_search_strategies(session: &Session) -> Value {
    json!(list_active_strategies(session))
}

// `web_search_set_engine`: set the session default engine used by `web_search` when a
// call omits `engine`. Validates the id resolves in the strategy registry (unknown →
// the registry's "unknown engine" error) before storing it. Returns `{engine,ok:true}`.
fn tool_search_set_engine(session: &mut Session, args: &Value) -> Result<Value, String> {
    let engine = arg_str(args, "engine").ok_or("web_search_set_engine: missing 'engine'")?;
    resolve_strategy(session, engine)?;
    session.default_search_engine = Some(engine.to_string());
    Ok(json!({ "engine": engine, "ok": true }))
}

// `web_search_load_strategy`: validate a strategy JSON object and register it as a
// session-scoped override for its engine id (enables new engines / hotfixes a stale
// built-in with no release). Returns `{engine,version,ok:true}`.
fn tool_search_load_strategy(session: &mut Session, args: &Value) -> Result<Value, String> {
    let raw = args
        .get("strategy")
        .ok_or("web_search_load_strategy: missing 'strategy' object")?;
    let strategy: Strategy = serde_json::from_value(raw.clone())
        .map_err(|e| format!("web_search_load_strategy: invalid strategy ({e})"))?;
    strategy.validate()?;
    let engine = strategy.engine.clone();
    let version = strategy.version.clone();
    session.search_overrides.insert(engine.clone(), strategy);
    Ok(json!({ "engine": engine, "version": version, "ok": true }))
}

// `web_search_reset_strategy`: drop one session override (`engine`) or clear all →
// back to the user-dir/built-in layers. Returns what was dropped.
fn tool_search_reset_strategy(session: &mut Session, args: &Value) -> Result<Value, String> {
    match arg_str(args, "engine") {
        Some(engine) => {
            let dropped = session.search_overrides.remove(engine).is_some();
            Ok(json!({ "engine": engine, "dropped": dropped }))
        }
        None => {
            let dropped: Vec<String> = session.search_overrides.keys().cloned().collect();
            session.search_overrides.clear();
            Ok(json!({ "dropped": dropped }))
        }
    }
}

// `web_search_setup_browser`: build the hardened-Chrome sidecar (installs patchright +
// verifies Chrome) so `web_search {browser:true}` / google works. Runs the committed
// setup script; `dir` overrides its location (default `scripts/browser-sidecar`,
// relative to the process cwd). Returns the script output + the env var to export.
async fn tool_setup_browser(args: &Value) -> Result<Value, String> {
    let dir = arg_str(args, "dir").unwrap_or("scripts/browser-sidecar");
    let script = format!("{dir}/setup.sh");
    if !std::path::Path::new(&script).exists() {
        return Err(format!(
            "setup script not found at {script} — pass `dir` pointing at the committed \
             scripts/browser-sidecar (or run `bash <dir>/setup.sh` manually)"
        ));
    }
    let out = tokio::process::Command::new("bash")
        .arg(&script)
        .output()
        .await
        .map_err(|e| format!("run {script}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let fetch_cmd = format!("node {dir}/fetch-serp.mjs");
    Ok(json!({
        "ok": out.status.success(),
        "output": stdout,
        "stderr": stderr,
        "set_env": { "TURBO_SURF_BROWSER_FETCH_CMD": fetch_cmd },
        "hint": "export TURBO_SURF_BROWSER_FETCH_CMD then web_search {engine:'google'} or browser:true",
    }))
}

pub fn tools() -> Value {
    let specs: &[(&str, &str)] = &[
        // navigation
        ("goto", "Fetch + parse a URL into the session"),
        ("reload", "Re-fetch the current URL"),
        ("go_back", "Navigate to the previous URL"),
        ("go_forward", "Navigate forward"),
        (
            "set_user_agent",
            "Set the User-Agent for subsequent fetches",
        ),
        // content / reads
        (
            "fetch_markdown",
            "One-shot goto+markdown: fetch a URL → {url,title,status,markdown}. \
             mode? (no-js|fast|secure) sets the JS tier; links? also returns \
             absolute links",
        ),
        (
            "fetch_markdown_batch",
            "Concurrent, order-preserving no-js batch of fetch_markdown: fetch \
             urls[] → [{url,title,status,markdown}] (one entry per input, a \
             failure as {url,error}). concurrency? (default 4); links? also \
             returns absolute links per page. No-js only — the render/JS tier is \
             single-page via fetch_markdown { mode }",
        ),
        (
            "web_search",
            "Web search via a one-shot SERP scrape → organic results \
             [{title,url,snippet?}] in rank order, driven by a data-defined parse \
             strategy (see web_search_strategies). engine? (default duckduckgo — the \
             most reliable no-JS endpoint; google/bing are best-effort scrapes that \
             may drift or captcha; searxng/baidu also bundled; or set a session \
             default via web_search_set_engine). base? is the instance URL for \
             engine:searxng. limit? (default 10). browser? routes the SERP fetch \
             through a real-browser sidecar (TURBO_SURF_BROWSER_FETCH_CMD) — needed \
             for engines behind a browser-integrity wall (google BotGuard). Stateless: \
             does not touch the session page/jar",
        ),
        (
            "web_search_set_engine",
            "Set the session default search engine used by web_search when a call \
             omits engine. Arg: engine (must resolve in the strategy registry — see \
             web_search_strategies). Unknown engine → error → {engine,ok}",
        ),
        (
            "web_search_strategies",
            "List the active search parse-strategies → \
             [{engine,version,source,format}] (source: custom|user-dir|built-in, \
             highest precedence first). Strategies are DATA, not code — dated + \
             overridable",
        ),
        (
            "web_search_load_strategy",
            "Register/override a search parse-strategy (session-scoped) for its \
             engine id. Arg: strategy (the JSON object: engine, version, query_url \
             with {query}/{limit}/{base}, format html|json, selectors or json_path). \
             Enables new engines or hotfixes a stale built-in with no release → \
             {engine,version,ok}",
        ),
        (
            "web_search_reset_strategy",
            "Drop a session strategy override (engine?) or clear all → back to the \
             user-dir/built-in layers. Returns what was dropped",
        ),
        (
            "web_search_setup_browser",
            "Build the hardened-Chrome sidecar (installs patchright + verifies Chrome) \
             so web_search {browser:true} / google works. Runs scripts/browser-sidecar/\
             setup.sh (override with dir?). Returns the output + the \
             TURBO_SURF_BROWSER_FETCH_CMD to export",
        ),
        (
            "markdown",
            "Markdown view of the current page's main content",
        ),
        ("text", "Plain-text view of the current page"),
        ("html", "Serialized HTML of the current page"),
        ("links", "Absolute http(s) links on the current page"),
        ("extract_links", "Absolute links (alias of links)"),
        ("interactive_elements", "Indexed interactive elements"),
        ("accessibility_tree", "Accessibility (role/name) tree"),
        ("aria_snapshot", "YAML-ish ARIA snapshot of <body>"),
        (
            "snapshot",
            "Combined orienting view (url/title/links/elements)",
        ),
        (
            "hydration_state",
            "No-JS hydration state (Next/JSON-LD/globals)",
        ),
        ("query", "Query by CSS or XPath"),
        ("get_by", "Locate by role/text/label/attr"),
        ("find_text", "Find elements containing text"),
        (
            "extract",
            "Structured extraction by a selector-bound schema",
        ),
        ("detect", "Lane B (JS-required) heuristic"),
        ("detect_js", "Lane B (JS-required) heuristic (alias)"),
        ("requests", "URLs fetched this session"),
        // interaction
        ("click", "Click an element (follow link / submit form)"),
        ("click_selector", "Click the first selector match (alias)"),
        ("submit", "Submit a form (selected, else the first form)"),
        ("fill", "Fill a control's value"),
        ("fill_selector", "Fill the first selector match (alias)"),
        (
            "fill_many",
            "Fill several controls from a {selector: value} map",
        ),
        ("check", "Check a checkbox/radio"),
        ("uncheck", "Uncheck a checkbox/radio"),
        ("select_option", "Select a <select> option by value/label"),
        // accessors (first selector match)
        ("get_attribute", "Attribute of the first selector match"),
        ("text_content", "Text content of the first selector match"),
        ("inner_html", "Inner HTML of the first selector match"),
        ("input_value", "Value of the first input match"),
        ("count", "Number of selector matches"),
        ("is_visible", "Visibility of the first selector match"),
        ("is_checked", "Checked state of the first selector match"),
        ("is_enabled", "Enabled state of the first selector match"),
        ("is_editable", "Editable state of the first selector match"),
        ("is_empty", "Emptiness of the first selector match"),
        ("is_focused", "Focus state (always false on a static DOM)"),
        ("aria_role", "ARIA role of the first selector match"),
        (
            "accessible_name",
            "Accessible name of the first selector match",
        ),
        (
            "accessible_description",
            "Accessible description of the first match",
        ),
        // render / JS tier
        (
            "probe",
            "Debug: run page JS with navigator/canvas instrumented; report what \
             fingerprinting code touched + what to shim",
        ),
        (
            "stealth_status",
            "Report the active fingerprint profile + whether a challenge solver is \
             wired + pool size",
        ),
        (
            "analyze_akamai",
            "EXPERIMENTAL: probe the live Akamai script on the current page, hash it, \
             and build candidate sensor_data per version. `{retry:true}` POSTs each \
             candidate, tests live acceptance, and saves a working one locally.",
        ),
        (
            "set_fingerprint",
            "Override render-tier navigator fields (JSON: userAgent, platform, \
             vendor, languages, hardwareConcurrency, deviceMemory, chromeMajor, \
             connection, userAgentData, screen, devicePixelRatio). {} resets.",
        ),
        ("set_mode", "Set JS render mode (no-js | fast | secure)"),
        (
            "render",
            "Run the page's own scripts (or a given script) + re-render",
        ),
        ("eval_js", "Evaluate JS against the current DOM → result"),
        (
            "evaluate",
            "Evaluate JS against the current DOM → result (alias)",
        ),
        (
            "inject_js",
            "Run JS that mutates the DOM; keep the hydrated result",
        ),
        ("latest_dom", "Most recent rendered HTML"),
        ("dom_history", "Rendered-HTML history trail"),
        (
            "screenshot",
            "Synthetic screenshot of the current page or a hydration-trail \
             snapshot — native layout+paint, no browser. Honors CSS \
             position/z-index stacking. Fetches the page's external <link> \
             stylesheets and its <img>/background-image bytes (via the session \
             client, so impersonation + cookies apply); PNG/JPEG/GIF/WebP/SVG \
             images are painted, others fall back to a placeholder. Args: format (png|svg), \
             snapshot? (dom_history index), width?/height? (override viewport), \
             external_css? (default true), images? (default true), \
             full_page? (default false — true grows height to full content). \
             PNG returns base64; SVG returns the document string.",
        ),
        (
            "set_viewport",
            "Set the default layout viewport (width/height px) used by screenshot",
        ),
        (
            "set_bypass_consent",
            "Toggle consent-wall bypass (google/youtube 'before you continue' \
             interstitial). On by default: seeds the consent cookie so the real \
             page is served. Arg: enabled? (default true)",
        ),
        (
            "run_playwright",
            "Execute a Playwright-style script (page/locator/getBy*/expect, test() blocks) with config (script, url?, testIdAttribute?) over the engine — no browser",
        ),
        // session / network
        ("get_cookies", "Cookie jar as a storageState array"),
        ("set_cookie", "Add a cookie to the jar"),
        ("set_extra_headers", "Set extra request headers"),
        ("robots_check", "robots.txt allow check for a URL"),
        ("fetch_json", "Fetch a URL and parse JSON (no navigation)"),
        (
            "fetch_raw",
            "Fetch a URL and return the raw body (no navigation)",
        ),
        // bulk
        ("crawl", "Crawl a site (BFS) → page records"),
        ("batch", "Fetch + parse a list of URLs concurrently"),
    ];
    let list: Vec<Value> = specs
        .iter()
        .map(|(name, desc)| {
            json!({
                "name": name,
                "description": desc,
                "inputSchema": { "type": "object", "properties": {}, "additionalProperties": true }
            })
        })
        .collect();
    json!({ "tools": list })
}

// --- tool dispatch ----------------------------------------------------------

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// A positive integer argument as `u32` (`0` and non-numbers yield `None`).
fn arg_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Value::as_u64)
        .filter(|&n| n > 0)
        .map(|n| n.min(u32::MAX as u64) as u32)
}

/// Run a tool by name, returning its result value (the caller wraps it in the
/// MCP `content` envelope).
pub async fn call_tool(session: &mut Session, name: &str, args: &Value) -> Result<Value, String> {
    let sel = || arg_str(args, "selector").ok_or_else(|| format!("{name}: missing 'selector'"));
    let val = || arg_str(args, "value").unwrap_or("").to_string();
    let script = || arg_str(args, "script").ok_or_else(|| format!("{name}: missing 'script'"));
    match name {
        // --- navigation ---
        "goto" => {
            session
                .goto(arg_str(args, "url").ok_or("goto: missing 'url'")?)
                .await
        }
        "fetch_markdown" => tool_fetch_markdown(session, args).await,
        "fetch_markdown_batch" => tool_fetch_markdown_batch(args).await,
        "web_search" => tool_search(session, args).await,
        "web_search_set_engine" => tool_search_set_engine(session, args),
        "web_search_strategies" => Ok(tool_search_strategies(session)),
        "web_search_load_strategy" => tool_search_load_strategy(session, args),
        "web_search_reset_strategy" => tool_search_reset_strategy(session, args),
        "web_search_setup_browser" => tool_setup_browser(args).await,
        "reload" => session.reload().await,
        "go_back" => session.go_back().await,
        "go_forward" => session.go_forward().await,
        "set_user_agent" => {
            session.ua = Some(val());
            Ok(json!({ "ok": true }))
        }
        // --- interaction ---
        "click" | "click_selector" => session.click(sel()?).await,
        "submit" => session.submit(arg_str(args, "selector")).await,
        "fill" | "fill_selector" => {
            let (s, v) = (sel()?.to_string(), val());
            session.mutate(&s, |t, h| view::fill_value(t, h, &v))
        }
        "fill_many" => tool_fill_many(session, args),
        "check" => {
            let s = sel()?.to_string();
            session.mutate(&s, |t, h| view::set_checked(t, h, true))
        }
        "uncheck" => {
            let s = sel()?.to_string();
            session.mutate(&s, |t, h| view::set_checked(t, h, false))
        }
        "select_option" => {
            let (s, v) = (sel()?.to_string(), val());
            session.mutate(&s, |t, h| {
                view::select_option(t, h, &v);
            })
        }
        // --- render / JS tier ---
        "set_mode" => {
            session.mode = arg_str(args, "mode").unwrap_or("no-js").to_string();
            Ok(json!({ "mode": session.mode }))
        }
        "set_bypass_consent" => {
            // Toggle consent-wall cookie seeding (google/youtube interstitial).
            // Default is on; pass `{ "enabled": false }` to fetch the raw response.
            let on = args.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            session.bypass_consent = Some(on);
            Ok(json!({ "bypassConsent": on }))
        }
        "eval_js" | "evaluate" => session.eval_js(script()?),
        "inject_js" => session.inject_js(script()?).await,
        "render" => match arg_str(args, "script") {
            Some(s) => session.inject_js(s).await,
            None => session
                .render_current()
                .await
                .map(|()| json!({ "ok": true })),
        },
        "probe" => session.probe().await,
        "analyze_akamai" => {
            session
                .analyze_akamai(args.get("retry").and_then(|v| v.as_bool()).unwrap_or(false))
                .await
        }
        "stealth_status" => Ok(session.stealth_status()),
        "set_fingerprint" => session.set_fingerprint(args.get("overrides").unwrap_or(args)),
        "latest_dom" => Ok(json!(session.dom_history.last())),
        "dom_history" => Ok(json!(session.dom_history)),
        "screenshot" => session.screenshot(args).await,
        "set_viewport" => session.set_viewport(args),
        "run_playwright" => tool_run_playwright(session, args).await,
        "requests" => Ok(json!(session.requests)),
        // --- session / network ---
        "set_extra_headers" => tool_set_headers(session, args),
        "get_cookies" => {
            serde_json::from_str(&session.jar.storage_state()).map_err(|e| e.to_string())
        }
        "set_cookie" => tool_set_cookie(session, args),
        "fetch_raw" => session
            .fetch_body(arg_str(args, "url").ok_or("fetch_raw: missing 'url'")?)
            .await
            .map(Value::String),
        "fetch_json" => {
            let body = session
                .fetch_body(arg_str(args, "url").ok_or("fetch_json: missing 'url'")?)
                .await?;
            serde_json::from_str(&body).map_err(|e| format!("invalid JSON: {e}"))
        }
        "robots_check" => tool_robots_check(session, args).await,
        // --- bulk ---
        "crawl" => tool_crawl(args).await,
        "batch" => tool_batch(args).await,
        _ => call_read_tool(session, name, args),
    }
}

fn call_read_tool(session: &mut Session, name: &str, args: &Value) -> Result<Value, String> {
    let tree = session.tree()?;
    let root = tree.root();
    let base = session.url.clone();
    match name {
        "markdown" => Ok(json!(view::markdown(tree, root, &base))),
        "text" => Ok(json!(view::text(tree, root))),
        "html" => Ok(json!(serialize_inner(tree, root))),
        "links" => Ok(json!(view::links(tree, &base))),
        "interactive_elements" => Ok(json!(view::interactive_elements(tree, &base, true))),
        "accessibility_tree" => Ok(json!(view::accessibility_tree(tree))),
        "aria_snapshot" => Ok(json!(aria_snapshot_body(tree))),
        "hydration_state" => Ok(json!(view::extract_hydration_state(tree))),
        "detect" => Ok(json!(view::detect_js_required(tree, None, None))),
        "detect_js" => Ok(json!(view::detect_js_required(tree, None, None))),
        "query" => tool_query(tree, root, args),
        "get_by" => tool_get_by(tree, args),
        "find_text" => tool_find_text(tree, args),
        "extract" => tool_extract(tree, &base, args),
        "extract_links" => Ok(json!(view::links(tree, &base))),
        "snapshot" => Ok(tool_snapshot(tree, &base)),
        "get_attribute" => tool_get_attribute(tree, args),
        "text_content" => Ok(json!(first(tree, args)?.map(|h| tree.text_content(h)))),
        "inner_html" => Ok(json!(first(tree, args)?.map(|h| serialize_inner(tree, h)))),
        "input_value" => Ok(json!(
            first(tree, args)?.map(|h| view::input_value_of(tree, h))
        )),
        "count" => Ok(json!(count_matches(tree, args)?)),
        "aria_role" => Ok(json!(first(tree, args)?.map(|h| view::role_of(tree, h)))),
        "accessible_name" => Ok(json!(
            first(tree, args)?.map(|h| view::accessible_name(tree, h))
        )),
        "accessible_description" => Ok(json!(
            first(tree, args)?.map(|h| view::accessible_description(tree, h))
        )),
        "is_visible" => Ok(json!(bool_accessor(tree, args, view::is_visible)?)),
        "is_checked" => Ok(json!(bool_accessor(tree, args, view::is_checked)?)),
        "is_enabled" => Ok(json!(bool_accessor(tree, args, view::is_enabled)?)),
        "is_editable" => Ok(json!(bool_accessor(tree, args, view::is_editable)?)),
        "is_empty" => Ok(json!(bool_accessor(tree, args, view::is_empty)?)),
        // no focus state on a static parsed DOM — honest constant.
        "is_focused" => Ok(json!(false)),
        _ => Err(format!("unknown tool: {name}")),
    }
}

// Apply a `(tree, handle) -> bool` view accessor to the first selector match
// (false when nothing matches).
fn bool_accessor(tree: &Tree, args: &Value, f: fn(&Tree, Handle) -> bool) -> Result<bool, String> {
    Ok(first(tree, args)?.is_some_and(|h| f(tree, h)))
}

fn count_matches(tree: &Tree, args: &Value) -> Result<usize, String> {
    let sel = arg_str(args, "selector").ok_or("count: missing 'selector'")?;
    Ok(tree.query_selector_all(sel).iter().count())
}

fn tool_find_text(tree: &Tree, args: &Value) -> Result<Value, String> {
    let text = arg_str(args, "text").ok_or("find_text: missing 'text'")?;
    let out: Vec<Value> = view::by_text(tree, text, TextMode::Substring)
        .iter()
        .map(|&h| json!({ "node": h.raw(), "text": view::text(tree, h) }))
        .collect();
    Ok(json!(out))
}

// A combined page snapshot (url + title + interactive elements + links) — the
// one-call orienting view an agent reaches for first.
fn tool_snapshot(tree: &Tree, base: &str) -> Value {
    json!({
        "url": base,
        "title": title_of(tree),
        "interactive_elements": view::interactive_elements(tree, base, true),
        "links": view::links(tree, base),
    })
}

// First selector match handle (or None), for accessor tools.
fn first(tree: &Tree, args: &Value) -> Result<Option<Handle>, String> {
    let sel = arg_str(args, "selector").ok_or("missing 'selector'")?;
    Ok(tree.query_selector(sel))
}

fn tool_get_attribute(tree: &Tree, args: &Value) -> Result<Value, String> {
    let name = arg_str(args, "name").ok_or("get_attribute: missing 'name'")?;
    let v = first(tree, args)?.and_then(|h| tree.get_attribute(h, name));
    Ok(json!(v))
}

fn aria_snapshot_body(tree: &Tree) -> String {
    match tree.query_selector("body") {
        Some(b) => view::aria_snapshot(tree, b),
        None => String::new(), // defensive: a parsed document always has <body>
    }
}

fn tool_query(tree: &Tree, root: Handle, args: &Value) -> Result<Value, String> {
    let selector = arg_str(args, "selector").ok_or("query: missing 'selector'")?;
    let ty = match arg_str(args, "type") {
        Some("css") => QueryType::Css,
        Some("xpath") => QueryType::Xpath,
        _ => QueryType::Auto,
    };
    Ok(json!(view::query(tree, root, selector, ty)))
}

fn tool_get_by(tree: &Tree, args: &Value) -> Result<Value, String> {
    let name = arg_str(args, "name").map(|n| (n, TextMode::Substring));
    let hits = if let Some(role) = arg_str(args, "role") {
        view::by_role(tree, role, name)
    } else if let Some(text) = arg_str(args, "text") {
        view::by_text(tree, text, TextMode::Substring)
    } else if let Some(label) = arg_str(args, "label") {
        view::by_label(tree, label, TextMode::Substring)
    } else {
        return Err("get_by: need one of role/text/label".to_string());
    };
    let out: Vec<Value> = hits
        .iter()
        .map(|&h| json!({ "node": h.raw(), "text": view::text(tree, h) }))
        .collect();
    Ok(json!(out))
}

// Parse a JSON schema object into the view Field map (selector/attr/type/list/fields).
fn parse_schema(v: &Value) -> BTreeMap<String, Field> {
    v.as_object()
        .map(|o| {
            o.iter()
                .map(|(k, spec)| (k.clone(), parse_field(spec)))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_field(spec: &Value) -> Field {
    Field {
        selector: arg_str(spec, "selector").map(str::to_string),
        attr: arg_str(spec, "attr").map(str::to_string),
        ftype: match arg_str(spec, "type") {
            Some("number") => FieldType::Number,
            Some("boolean") => FieldType::Boolean,
            _ => FieldType::String,
        },
        list: spec.get("list").and_then(Value::as_bool).unwrap_or(false),
        fields: spec.get("fields").map(parse_schema),
    }
}

fn tool_extract(tree: &Tree, base: &str, args: &Value) -> Result<Value, String> {
    let schema = args.get("schema").ok_or("extract: missing 'schema'")?;
    Ok(view::extract_schema(tree, &parse_schema(schema), base))
}

fn tool_fill_many(session: &mut Session, args: &Value) -> Result<Value, String> {
    let map = args
        .get("values")
        .and_then(Value::as_object)
        .ok_or("fill_many: missing 'values' object")?;
    let pairs: Vec<(String, String)> = map
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect();
    for (s, v) in &pairs {
        session.mutate(s, |t, h| view::fill_value(t, h, v))?;
    }
    Ok(json!({ "filled": pairs.len() }))
}

fn tool_set_headers(session: &mut Session, args: &Value) -> Result<Value, String> {
    let map = args
        .get("headers")
        .and_then(Value::as_object)
        .ok_or("set_extra_headers: missing 'headers' object")?;
    for (k, v) in map {
        if let Some(s) = v.as_str() {
            session.headers.insert(k.clone(), s.to_string());
        }
    }
    Ok(json!({ "ok": true }))
}

fn tool_set_cookie(session: &mut Session, args: &Value) -> Result<Value, String> {
    let name = arg_str(args, "name").ok_or("set_cookie: missing 'name'")?;
    let value = arg_str(args, "value").unwrap_or("");
    let domain = arg_str(args, "domain").unwrap_or("");
    let path = arg_str(args, "path").unwrap_or("/");
    let expires = args.get("expires").and_then(Value::as_f64);
    session.jar.add(name, value, domain, path, expires);
    Ok(json!({ "ok": true }))
}

// Net-backed robots fetcher (the trait ships only test stubs in core).
struct NetFetcher;
#[async_trait::async_trait]
impl RobotsFetcher for NetFetcher {
    async fn fetch_text(&self, url: &str) -> Result<(u16, String), ()> {
        let opts = FetchOptions {
            allow_non_html: true,
            ..Default::default()
        };
        fetch_html(url, opts)
            .await
            .map(|r| (r.status, r.html))
            .map_err(|_| ())
    }
}

async fn tool_robots_check(session: &Session, args: &Value) -> Result<Value, String> {
    let url = arg_str(args, "url").unwrap_or(&session.url);
    if url.is_empty() {
        return Err("robots_check: missing 'url'".to_string());
    }
    let ua = session.ua.as_deref().unwrap_or("turbo-surf");
    let mut cache = RobotsCache::new(NetFetcher);
    let allowed = cache.allowed(url, ua, 0).await;
    Ok(json!({ "url": url, "allowed": allowed }))
}

fn crawl_options(args: &Value) -> CrawlOptions {
    let start = match args.get("start") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => arg_str(args, "url")
            .map(|u| vec![u.to_string()])
            .unwrap_or_default(),
    };
    let u = |k: &str, d: u64| args.get(k).and_then(Value::as_u64).unwrap_or(d);
    CrawlOptions {
        start,
        max_pages: u("maxPages", 50) as usize,
        max_depth: u("maxDepth", 3) as usize,
        concurrency: u("concurrency", 4) as usize,
        same_host_only: args
            .get("sameHost")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        ..Default::default()
    }
}

async fn tool_crawl(args: &Value) -> Result<Value, String> {
    let item_selector = arg_str(args, "itemSelector").map(str::to_string);
    let nav = TurboNavigator::default().with_item_selector(item_selector);
    let recs = run_crawl(crawl_options(args), std::sync::Arc::new(nav)).await;
    let out: Vec<Value> = recs
        .iter()
        .map(|r| json!({ "url": r.url, "status": r.status, "title": r.title, "items": r.items, "error": r.error }))
        .collect();
    Ok(json!(out))
}

async fn tool_batch(args: &Value) -> Result<Value, String> {
    let urls: Vec<String> = args
        .get("urls")
        .and_then(Value::as_array)
        .ok_or("batch: missing 'urls' array")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let concurrency = args.get("concurrency").and_then(Value::as_u64).unwrap_or(4) as usize;
    let results = batch_urls(&TurboNavigator::default(), urls, concurrency).await;
    let out: Vec<Value> = results
        .iter()
        .map(|(url, r)| match r {
            Ok(nav) => json!({ "url": url, "status": nav.status, "title": nav.title }),
            Err(e) => json!({ "url": url, "error": e }),
        })
        .collect();
    Ok(json!(out))
}

// --- JSON-RPC envelope ------------------------------------------------------

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: Value, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } })
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "turbo-surf", "version": VERSION }
    })
}

async fn tools_call(session: &mut Session, params: &Value) -> Result<Value, String> {
    let name = arg_str(params, "name").ok_or("tools/call: missing 'name'")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let value = call_tool(session, name, &args).await?;
    // MCP content envelope: a single text block carrying the serialized result.
    let text = match &value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

/// Handle one JSON-RPC request object, returning the response object (or `None`
/// for a notification, which has no `id`).
pub async fn handle(session: &mut Session, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    // Notifications (no id) get no response.
    id.as_ref()?;
    let id = id.unwrap();

    let result = match method {
        "initialize" => Ok(initialize_result()),
        "tools/list" => Ok(tools()),
        "tools/call" => tools_call(session, &params).await,
        other => Err(format!("unknown method: {other}")),
    };
    Some(match result {
        Ok(r) => ok(id, r),
        Err(e) => err(id, &e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "<html><head><title>T</title></head><body>\
        <main><h1>Hi</h1><p>para</p></main>\
        <a href='/x'>L</a><button>Go</button>\
        <div id='app'></div><script src='/a.js'></script>\
        <script id='__NEXT_DATA__' type='application/json'>{\"p\":1}</script>\
        </body></html>";

    fn loaded() -> Session {
        let mut s = Session::new();
        s.load("https://x.test/", PAGE);
        s
    }

    async fn call(s: &mut Session, name: &str, args: Value) -> Value {
        call_tool(s, name, &args).await.unwrap()
    }

    // The browser-sidecar contract (env-independent core): a stub `node -e` command
    // stands in for the real chromium sidecar — it drains stdin and prints a JSON
    // {html} line. Verifies we drive the sidecar and read its html back. (The JS has
    // no spaces so it survives the command's whitespace split into program + args.)
    #[tokio::test]
    async fn browser_serp_reads_sidecar_html() {
        let html = browser_fetch_serp_cmd(
            "https://e/",
            "node -e process.stdin.resume();process.stdout.write(JSON.stringify({html:'<h3>ok</h3>'}))",
        )
        .await;
        assert_eq!(html.unwrap(), "<h3>ok</h3>");
    }

    #[tokio::test]
    async fn browser_serp_surfaces_captcha_block() {
        let err = browser_fetch_serp_cmd(
            "https://e/",
            "node -e process.stdin.resume();process.stdout.write(JSON.stringify({blocked:true,finalUrl:'https://g/sorry'}))",
        )
        .await
        .unwrap_err();
        assert!(err.contains("captcha"), "block surfaced: {err}");
    }

    #[tokio::test]
    async fn read_tools_over_loaded_page() {
        let mut s = loaded();
        assert!(call(&mut s, "markdown", json!({}))
            .await
            .as_str()
            .unwrap()
            .contains("# Hi"));
        assert!(call(&mut s, "text", json!({}))
            .await
            .as_str()
            .unwrap()
            .contains("para"));
        assert!(call(&mut s, "html", json!({}))
            .await
            .as_str()
            .unwrap()
            .contains("<h1>"));
        assert_eq!(
            call(&mut s, "links", json!({})).await,
            json!(["https://x.test/x"])
        );
        assert_eq!(
            call(&mut s, "interactive_elements", json!({}))
                .await
                .as_array()
                .unwrap()
                .len(),
            2
        );
        // body has several roled children → a generic wrapper containing them
        let ax = call(&mut s, "accessibility_tree", json!({})).await;
        assert_eq!(ax["role"], "generic");
        assert!(ax.to_string().contains("\"main\""));
    }

    #[tokio::test]
    async fn structured_and_locator_tools() {
        let mut s = loaded();
        // query (CSS)
        let q = call(&mut s, "query", json!({ "selector": "h1" })).await;
        assert_eq!(q[0]["text"], "Hi");
        // get_by role
        let g = call(&mut s, "get_by", json!({ "role": "button" })).await;
        assert_eq!(g[0]["text"], "Go");
        // extract schema
        let e = call(
            &mut s,
            "extract",
            json!({ "schema": { "heading": { "selector": "h1" } } }),
        )
        .await;
        assert_eq!(e["heading"], "Hi");
        // hydration + detect
        assert_eq!(
            call(&mut s, "hydration_state", json!({})).await["next"],
            json!({"p": 1})
        );
        assert_eq!(call(&mut s, "detect", json!({})).await["js_required"], true);
    }

    #[tokio::test]
    async fn jsonrpc_envelope() {
        let mut s = loaded();
        // initialize
        let init = handle(
            &mut s,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        )
        .await
        .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "turbo-surf");
        // tools/list
        let list = handle(
            &mut s,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .await
        .unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().len() >= 13);
        // tools/call → content envelope
        let call = handle(
            &mut s,
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"text","arguments":{}}}),
        )
        .await
        .unwrap();
        assert!(call["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Hi"));
        // a non-string tool result is JSON-serialized into the text block
        let links = handle(
            &mut s,
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"links","arguments":{}}}),
        )
        .await
        .unwrap();
        assert!(links["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with('['));
        // notification (no id) → no response
        assert!(handle(&mut s, &json!({"jsonrpc":"2.0","method":"x"}))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn set_fingerprint_overrides_render_navigator() {
        let mut s = Session::new();
        let r = call(
            &mut s,
            "set_fingerprint",
            json!({ "overrides": { "platform": "Win32", "hardwareConcurrency": 16 } }),
        )
        .await;
        assert_eq!(r["ok"], true);
        // Persisted on the session + reflected by stealth_status. (The JS
        // application of the override is covered in the render crate; eval_js here
        // would hit the per-thread cached isolate and race.)
        let st = call(&mut s, "stealth_status", json!({})).await;
        assert_eq!(st["renderFingerprintOverrides"]["platform"], "Win32");
        assert_eq!(st["renderFingerprintOverrides"]["hardwareConcurrency"], 16);
        // Reset the process-global for other tests.
        turbo_surf_render::set_fingerprint("{}");
    }

    #[tokio::test]
    async fn goto_fetches_and_loads_over_localhost() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                // Drain the whole request before replying: a real Chrome header
                // set is ~600 B, so a 512-B buffer left bytes unread and the
                // close-after-write RST-truncated the response on the client.
                let mut b = [0u8; 2048];
                let _ = sock.read(&mut b).await;
                let body = "<html><head><title>Live</title></head><body><p>hello</p></body></html>";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{body}"
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        let mut s = Session::new();
        let r = call_tool(
            &mut s,
            "goto",
            &json!({ "url": format!("http://127.0.0.1:{port}/") }),
        )
        .await
        .unwrap();
        assert_eq!(r["status"], 200);
        assert_eq!(r["title"], "Live");
        // session now serves read tools
        assert!(call(&mut s, "text", json!({}))
            .await
            .as_str()
            .unwrap()
            .contains("hello"));
    }

    #[tokio::test]
    async fn fetch_markdown_one_shot_over_localhost() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let mut b = [0u8; 2048];
                let _ = sock.read(&mut b).await;
                let body = "<html><head><title>Doc</title></head><body>\
                    <main><h1>Hi</h1><p>para</p></main>\
                    <a href='/x'>L</a></body></html>";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{body}"
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        let url = format!("http://127.0.0.1:{port}/");
        // Bare call: fetch + markdown in one, no `links` field.
        let mut s = Session::new();
        let r = call(&mut s, "fetch_markdown", json!({ "url": url })).await;
        assert_eq!(r["url"], url);
        assert_eq!(r["status"], 200);
        assert_eq!(r["title"], "Doc");
        assert!(r["markdown"].as_str().unwrap().contains("# Hi"));
        assert!(r.get("links").is_none());
        // links:true adds the absolute-links array.
        let r = call(
            &mut s,
            "fetch_markdown",
            json!({ "url": url, "links": true }),
        )
        .await;
        assert_eq!(r["links"], json!([format!("{url}x")]));
    }

    #[tokio::test]
    async fn fetch_markdown_batch_over_localhost() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut b = [0u8; 2048];
                    let n = sock.read(&mut b).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&b[..n]);
                    // Serve a distinct titled page per path so we can assert the
                    // result array is order-preserving across concurrent fetches.
                    let title = if req.contains("/two") { "Two" } else { "One" };
                    let body = format!(
                        "<html><head><title>{title}</title></head><body>\
                         <main><h1>{title}</h1><p>para</p></main>\
                         <a href='/x'>L</a></body></html>"
                    );
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{body}"
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        let one = format!("http://127.0.0.1:{port}/one");
        let two = format!("http://127.0.0.1:{port}/two");
        // Port 1 refuses the connection → the per-URL fetch errors out.
        let dead = "http://127.0.0.1:1/gone".to_string();
        let out = call(
            &mut Session::new(),
            "fetch_markdown_batch",
            json!({ "urls": [one, two, dead], "links": true }),
        )
        .await;
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        // Order-preserving: one, two, then the failed entry — regardless of which
        // concurrent fetch finished first.
        assert_eq!(arr[0]["url"], one);
        assert_eq!(arr[0]["status"], 200);
        assert_eq!(arr[0]["title"], "One");
        assert!(arr[0]["markdown"].as_str().unwrap().contains("# One"));
        // `/x` resolves against the host root, not the `/one` path.
        assert_eq!(
            arr[0]["links"],
            json!([format!("http://127.0.0.1:{port}/x")])
        );
        assert_eq!(arr[1]["url"], two);
        assert_eq!(arr[1]["title"], "Two");
        assert!(arr[1]["markdown"].as_str().unwrap().contains("# Two"));
        // A per-URL failure is captured, not fatal: `{url,error}`, no markdown.
        assert_eq!(arr[2]["url"], dead);
        assert!(arr[2].get("error").is_some());
        assert!(arr[2].get("markdown").is_none());
    }

    // E2E of the whole challenge pipeline (detect → solve → inject cookie →
    // replay) over localhost, no real browser/network: a server that walls the
    // first hit and serves the page once a "cleared" cookie is present, plus a
    // fake sidecar (BrowserSolver shelling to `printf`) that returns that cookie.
    #[tokio::test]
    async fn e2e_solver_clears_wall_and_replays() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut b = [0u8; 4096];
                    let n = sock.read(&mut b).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&b[..n]);
                    let cleared = req.lines().any(|l| {
                        l.to_ascii_lowercase().starts_with("cookie:") && l.contains("cleared=1")
                    });
                    let resp = if cleared {
                        let body = "<html><head><title>Real</title></head><body>ok</body></html>";
                        format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    } else {
                        // Challenge wall: a body marker the detector keys on.
                        let body =
                            "<html><body>/cdn-cgi/challenge-platform/ checking…</body></html>";
                        format!("HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nSet-Cookie: datadome=chal; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        // Fake hardened-headless sidecar: "solves" by returning the gating cookie.
        let solver = Box::new(turbo_surf_core::challenge::BrowserSolver::new(
            "cat >/dev/null; printf '{\"cookies\":{\"cleared\":\"1\"}}'".into(),
        ));
        let mut s = Session::with_solver(solver);
        let out = s.goto(&format!("http://127.0.0.1:{port}/")).await.unwrap();
        // Walled 403 → detected → solved → re-fetched → real page.
        assert_eq!(out["status"], 200, "expected solved page, got {out}");
        assert_eq!(out["title"], "Real");
    }

    // E2E with the REAL in-house AkamaiSolver (not a stub): an Akamai-walled
    // localhost site — 403 + `_abck` seed until a sensor POST clears it — driven
    // through the whole MCP session pipeline (detect → AkamaiSolver.solve → inject
    // `_abck` → replay → real page).
    #[tokio::test]
    async fn e2e_akamai_solver_clears_wall() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut b = vec![0u8; 8192];
                    let n = sock.read(&mut b).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&b[..n]);
                    let resp = if req.starts_with("POST") {
                        // Sensor accepted → issue a cleared _abck.
                        let body = "{}";
                        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: _abck=CLEARED~0~ok; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    } else if req.contains("_abck=CLEARED") {
                        let body = "<html><head><title>Real</title></head><body>ok</body></html>";
                        format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    } else {
                        // Wall: seed _abck so the detector fires Akamai.
                        let body = "<html><body>bot wall</body></html>";
                        format!("HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nSet-Cookie: _abck=0~seed~-1~-1; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        let mut s = Session::with_solver(Box::new(turbo_surf_core::akamai::AkamaiSolver::new()));
        let out = s.goto(&format!("http://127.0.0.1:{port}/")).await.unwrap();
        assert_eq!(out["status"], 200, "akamai not cleared: {out}");
        assert_eq!(out["title"], "Real");
    }

    // E2E with the REAL in-house CloudflareSolver: a managed-challenge localhost
    // site — interstitial until the challenge POST issues `cf_clearance`.
    #[tokio::test]
    async fn e2e_cloudflare_solver_clears_wall() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        const INTERSTITIAL: &str = "<html><head><script>window._cf_chl_opt={cvId:'3',cRay:'8af0deadbeef'};</script></head><body>/cdn-cgi/challenge-platform/ checking…</body></html>";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut b = vec![0u8; 8192];
                    let n = sock.read(&mut b).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&b[..n]);
                    let resp = if req.starts_with("POST") {
                        let body = "{}";
                        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: cf_clearance=CF~cleared~1; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    } else if req.contains("cf_clearance=CF") {
                        let body = "<html><head><title>Real</title></head><body>ok</body></html>";
                        format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    } else {
                        format!("HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{INTERSTITIAL}", INTERSTITIAL.len())
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        let mut s = Session::with_solver(Box::new(
            turbo_surf_core::cloudflare::CloudflareSolver::new(),
        ));
        let out = s.goto(&format!("http://127.0.0.1:{port}/")).await.unwrap();
        assert_eq!(out["status"], 200, "cloudflare not cleared: {out}");
        assert_eq!(out["title"], "Real");
    }

    #[tokio::test]
    async fn aria_query_getby_branches() {
        let mut s = loaded();
        assert!(call(&mut s, "aria_snapshot", json!({}))
            .await
            .as_str()
            .unwrap()
            .contains("- "));
        // explicit query types
        assert_eq!(
            call(&mut s, "query", json!({"selector":"h1","type":"css"})).await[0]["text"],
            "Hi"
        );
        assert_eq!(
            call(&mut s, "query", json!({"selector":"//h1","type":"xpath"})).await[0]["text"],
            "Hi"
        );
        // get_by text + label (label absent → empty list, exercises the branch)
        assert!(!call(&mut s, "get_by", json!({"text":"para"}))
            .await
            .as_array()
            .unwrap()
            .is_empty());
        assert!(call(&mut s, "get_by", json!({"label":"none"}))
            .await
            .as_array()
            .unwrap()
            .is_empty());
        // missing-arg errors
        assert!(call_tool(&mut s, "query", &json!({})).await.is_err());
        assert!(call_tool(&mut s, "get_by", &json!({})).await.is_err());
        assert!(call_tool(&mut s, "extract", &json!({})).await.is_err());
    }

    #[tokio::test]
    async fn action_tools_mutate_and_read() {
        let mut s = Session::new();
        s.load(
            "https://x.test/",
            "<input id='t'><input id='c' type='checkbox'><a id='x' href='/p'>l</a><div id='d' style='display:none'>x</div>",
        );
        // fill + check mutate the session tree
        call(
            &mut s,
            "fill",
            json!({ "selector": "#t", "value": "typed" }),
        )
        .await;
        call(&mut s, "check", json!({ "selector": "#c" })).await;
        // accessor reads reflect the mutations
        assert_eq!(
            call(
                &mut s,
                "get_attribute",
                json!({ "selector": "#t", "name": "value" })
            )
            .await,
            "typed"
        );
        assert_eq!(
            call(&mut s, "is_visible", json!({ "selector": "#d" })).await,
            false
        );
        assert_eq!(
            call(&mut s, "is_visible", json!({ "selector": "#x" })).await,
            true
        );
    }

    #[tokio::test]
    async fn click_link_and_history() {
        // link click → navigate; go_back returns to the origin.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                // Drain the whole request before replying: a real Chrome header
                // set is ~600 B, so a 512-B buffer left bytes unread and the
                // close-after-write RST-truncated the response on the client.
                let mut b = [0u8; 2048];
                let _ = sock.read(&mut b).await;
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<title>Dest</title>";
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        let mut s = Session::new();
        s.load(
            &format!("http://127.0.0.1:{port}/"),
            &format!("<a href='http://127.0.0.1:{port}/next'>go</a>"),
        );
        let clicked = call_tool(&mut s, "click", &json!({ "selector": "a" }))
            .await
            .unwrap();
        assert_eq!(clicked["title"], "Dest");
        // go_back to the origin
        let back = call_tool(&mut s, "go_back", &json!({})).await.unwrap();
        assert!(back["url"].as_str().unwrap().ends_with("/"));
    }

    #[tokio::test]
    async fn accessor_and_aggregate_tools() {
        let mut s = Session::new();
        s.load(
            "https://x.test/",
            "<main><h1 id='t'>Hi</h1><input id='i' value='v'><input type='checkbox' id='c' checked>\
             <p class='q'>one</p><p class='q'>two</p><a href='/a'>L</a></main>",
        );
        // count
        assert_eq!(call(&mut s, "count", json!({ "selector": ".q" })).await, 2);
        // text_content / input_value / aria_role / accessible_name
        assert_eq!(
            call(&mut s, "text_content", json!({ "selector": "#t" })).await,
            "Hi"
        );
        assert_eq!(
            call(&mut s, "input_value", json!({ "selector": "#i" })).await,
            "v"
        );
        assert_eq!(
            call(&mut s, "aria_role", json!({ "selector": "a" })).await,
            "link"
        );
        // is_checked
        assert_eq!(
            call(&mut s, "is_checked", json!({ "selector": "#c" })).await,
            true
        );
        assert_eq!(
            call(&mut s, "is_focused", json!({ "selector": "#t" })).await,
            false
        );
        // find_text → matches; extract_links alias
        assert!(!call(&mut s, "find_text", json!({ "text": "one" }))
            .await
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            call(&mut s, "extract_links", json!({})).await,
            json!(["https://x.test/a"])
        );
        // snapshot aggregate
        let snap = call(&mut s, "snapshot", json!({})).await;
        assert_eq!(snap["url"], "https://x.test/");
        assert!(snap["links"]
            .as_array()
            .unwrap()
            .contains(&json!("https://x.test/a")));
        // detect_js alias
        assert!(call(&mut s, "detect_js", json!({}))
            .await
            .get("js_required")
            .is_some());
    }

    #[tokio::test]
    async fn fill_many_mode_cookies_requests() {
        let mut s = Session::new();
        s.load("https://x.test/", "<input id='a'><input id='b'>");
        call(
            &mut s,
            "fill_many",
            json!({ "values": { "#a": "1", "#b": "2" } }),
        )
        .await;
        assert_eq!(
            call(&mut s, "input_value", json!({ "selector": "#a" })).await,
            "1"
        );
        assert_eq!(
            call(&mut s, "input_value", json!({ "selector": "#b" })).await,
            "2"
        );
        // mode toggle
        assert_eq!(
            call(&mut s, "set_mode", json!({ "mode": "fast" })).await["mode"],
            "fast"
        );
        // cookie set → get_cookies reflects it
        call(
            &mut s,
            "set_cookie",
            json!({ "name": "k", "value": "v", "domain": "x.test" }),
        )
        .await;
        assert!(call(&mut s, "get_cookies", json!({}))
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "k"));
        // user-agent + extra headers don't error
        call(&mut s, "set_user_agent", json!({ "value": "Bot/9" })).await;
        call(
            &mut s,
            "set_extra_headers",
            json!({ "headers": { "X-Test": "1" } }),
        )
        .await;
    }

    #[tokio::test]
    async fn eval_js_over_loaded_dom() {
        let mut s = loaded();
        let r = call(
            &mut s,
            "eval_js",
            json!({ "script": "document.querySelector('h1').textContent" }),
        )
        .await;
        assert_eq!(r, "Hi");
        // evaluate is an alias
        assert_eq!(
            call(
                &mut s,
                "evaluate",
                json!({ "script": "String(document.querySelectorAll('a').length)" })
            )
            .await,
            "1"
        );
    }

    #[tokio::test]
    async fn errors_surface() {
        let mut s = loaded();
        // unknown method
        let e = handle(&mut s, &json!({"jsonrpc":"2.0","id":1,"method":"bogus"}))
            .await
            .unwrap();
        assert!(e["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown method"));
        // unknown tool
        assert!(call_tool(&mut s, "nope", &json!({})).await.is_err());
        // read tool with no page loaded
        let mut empty = Session::new();
        assert!(call_tool(&mut empty, "text", &json!({})).await.is_err());
        // goto missing url
        assert!(call_tool(&mut s, "goto", &json!({})).await.is_err());
    }

    // --- web search (SERP parser) -------------------------------------------
    // Saved SERP snippets mimicking each engine's real structure (2-3 organic
    // results). The parser is a PURE fn so these need no network.

    // DDG html endpoint: `.web-result` containers, `a.result__a` title/link whose
    // href is the `//duckduckgo.com/l/?uddg=<enc>` redirect wrapper, and
    // `.result__snippet`. Second result is a bare (already-direct) href.
    const DDG_HTML: &str = r#"<html><body>
      <div class="result results_links web-result">
        <h2 class="result__title">
          <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fone&amp;rut=abc">First Result</a>
        </h2>
        <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fone">Snippet one text.</a>
      </div>
      <div class="result results_links web-result">
        <h2 class="result__title">
          <a class="result__a" href="https://direct.example.org/two">Second Result</a>
        </h2>
        <a class="result__snippet">Snippet two text.</a>
      </div>
    </body></html>"#;

    const BING_HTML: &str = r#"<html><body>
      <ol id="b_results">
        <li class="b_algo"><h2><a href="https://example.com/b1">Bing One</a></h2>
          <div class="b_caption"><p>Bing snippet one.</p></div></li>
        <li class="b_algo"><h2><a href="https://example.com/b2">Bing Two</a></h2>
          <div class="b_caption"><p>Bing snippet two.</p></div></li>
      </ol>
    </body></html>"#;

    const GOOGLE_HTML: &str = r#"<html><body>
      <div class="g"><div class="yuRUbf"><a href="https://example.com/g1"><h3>Google One</h3></a></div>
        <div class="VwiC3b">Google snippet one.</div></div>
      <div class="g"><div class="yuRUbf"><a href="https://example.com/g2"><h3>Google Two</h3></a></div>
        <div class="VwiC3b">Google snippet two.</div></div>
    </body></html>"#;

    const SEARXNG_JSON: &str = r#"{"query":"rust","results":[
      {"title":"Sx One","url":"https://example.com/s1","content":"Sx snippet one."},
      {"title":"Sx Two","url":"https://example.com/s2","content":"Sx snippet two."},
      {"title":"No URL","url":"","content":"dropped"}
    ]}"#;

    // Parse a strategy from a JSON literal — the same path `web_search_load_strategy`
    // and the bundled defaults take, so tests exercise the real interpreter.
    fn strat(json: &str) -> Strategy {
        serde_json::from_str(json).expect("test strategy should parse")
    }

    // The bundled built-in strategies, by engine id.
    fn built_in() -> HashMap<String, Strategy> {
        strategies_from_json_array(BUNDLED_STRATEGIES_JSON)
    }

    #[test]
    fn search_parses_duckduckgo_and_decodes_uddg() {
        let r = parse_serp(built_in().get("duckduckgo").unwrap(), DDG_HTML, 10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "First Result");
        // The `uddg` redirect wrapper is decoded to the real absolute URL.
        assert_eq!(r[0].url, "https://example.com/one");
        assert_eq!(r[0].snippet.as_deref(), Some("Snippet one text."));
        // A bare (non-wrapped) href is used as-is.
        assert_eq!(r[1].url, "https://direct.example.org/two");
    }

    #[test]
    fn search_parses_bing() {
        let r = parse_serp(built_in().get("bing").unwrap(), BING_HTML, 10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "Bing One");
        assert_eq!(r[0].url, "https://example.com/b1");
        assert_eq!(r[1].snippet.as_deref(), Some("Bing snippet two."));
    }

    #[test]
    fn search_parses_google_structural() {
        // Google uses the classname-free structural parser (its result classes are
        // obfuscated + rotate): an <a href="http…"> containing an <h3> is a result.
        let r = parse_serp(built_in().get("google").unwrap(), GOOGLE_HTML, 10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "Google One");
        assert_eq!(r[0].url, "https://example.com/g1");
        assert_eq!(r[1].url, "https://example.com/g2");
        // structural extraction carries no snippet.
        assert!(r[0].snippet.is_none());
    }

    #[test]
    fn structural_skips_internal_hosts_and_dedups() {
        // Only off-site anchors that WRAP an <h3> count; google-internal nav (google.com
        // /gstatic), an <h3>-less link, and a duplicate URL are all filtered.
        let html = r#"<html><body>
          <a href="https://www.google.com/search?q=x"><h3>internal nav</h3></a>
          <a href="https://maps.gstatic.com/a"><h3>asset</h3></a>
          <a href="https://ex.com/1"><h3>Real One</h3></a>
          <a href="https://ex.com/nav">no h3 here</a>
          <a href="https://ex.com/1"><h3>Dup URL</h3></a>
          <a href="https://ex.com/2"><h3>Real Two</h3></a>
        </body></html>"#;
        let google = strat(
            r#"{"engine":"google","query_url":"https://g/?q={query}","format":"structural"}"#,
        );
        let r = parse_serp(&google, html, 10);
        assert_eq!(r.len(), 2, "internal/no-h3/dup filtered: {r:?}");
        assert_eq!(r[0].title, "Real One");
        assert_eq!(r[0].url, "https://ex.com/1");
        assert_eq!(r[1].url, "https://ex.com/2");
    }

    #[test]
    fn search_parses_searxng_json_and_skips_empty_url() {
        let r = parse_serp(built_in().get("searxng").unwrap(), SEARXNG_JSON, 10);
        // The url-less third entry is dropped.
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "Sx One");
        assert_eq!(r[0].url, "https://example.com/s1");
        assert_eq!(r[0].snippet.as_deref(), Some("Sx snippet one."));
    }

    #[test]
    fn search_respects_limit() {
        let reg = built_in();
        assert_eq!(parse_serp(reg.get("bing").unwrap(), BING_HTML, 1).len(), 1);
        assert_eq!(
            parse_serp(reg.get("searxng").unwrap(), SEARXNG_JSON, 1).len(),
            1
        );
    }

    #[test]
    fn build_query_url_interpolates_and_searxng_requires_base() {
        let reg = built_in();
        assert_eq!(
            build_query_url(reg.get("duckduckgo").unwrap(), "rust lang", 10, None).unwrap(),
            "https://html.duckduckgo.com/html/?q=rust+lang"
        );
        assert_eq!(
            build_query_url(reg.get("google").unwrap(), "rust", 5, None).unwrap(),
            "https://www.google.com/search?q=rust"
        );
        assert_eq!(
            build_query_url(
                reg.get("searxng").unwrap(),
                "rust",
                10,
                Some("http://localhost:8888/")
            )
            .unwrap(),
            "http://localhost:8888/search?q=rust&format=json"
        );
        // `{base}` present but no base supplied → error.
        assert!(build_query_url(reg.get("searxng").unwrap(), "rust", 10, None).is_err());
    }

    // The bundled default JSON must deserialize + carry all five engines, each
    // valid + dated — a malformed default fails the gate here.
    #[test]
    fn bundled_strategies_parse_validate_and_cover_five_engines() {
        // Deserializes as a whole (a syntax/shape error would fail here).
        let all: Vec<Strategy> =
            serde_json::from_str(BUNDLED_STRATEGIES_JSON).expect("bundled JSON should deserialize");
        assert_eq!(all.len(), 5, "expected 5 bundled engines");
        let reg = built_in();
        for engine in ["duckduckgo", "bing", "google", "searxng", "baidu"] {
            let s = reg
                .get(engine)
                .unwrap_or_else(|| panic!("bundled default missing '{engine}'"));
            s.validate()
                .unwrap_or_else(|e| panic!("bundled '{engine}' invalid: {e}"));
            assert!(!s.version.is_empty(), "'{engine}' needs a version date");
            assert!(s.query_url.contains("{query}"));
        }
    }

    // An unknown engine (no override, not in user-dir/built-in) resolves to a
    // helpful error naming the load tool.
    #[test]
    fn resolve_unknown_engine_errors() {
        let s = Session::new();
        let e = resolve_strategy(&s, "bogus").unwrap_err();
        assert!(e.contains("unknown engine 'bogus'"), "{e}");
        assert!(e.contains("web_search_load_strategy"), "{e}");
    }

    // `web_search_load_strategy` override wins over the built-in; `web_search_reset_strategy`
    // restores it.
    #[tokio::test]
    async fn load_strategy_overrides_builtin_then_reset_restores() {
        let mut s = Session::new();
        // Default: duckduckgo resolves to the built-in.
        assert_eq!(resolve_strategy(&s, "duckduckgo").unwrap().1, "built-in");
        // Load a session override for the same engine id.
        let r = call_tool(
            &mut s,
            "web_search_load_strategy",
            &json!({ "strategy": {
                "engine": "duckduckgo",
                "version": "2099-01-01",
                "query_url": "https://override.test/?q={query}",
                "format": "html",
                "result_container": ".r",
                "title": "a",
                "link": "a"
            }}),
        )
        .await
        .unwrap();
        assert_eq!(r["ok"], true);
        assert_eq!(r["version"], "2099-01-01");
        // Now the override wins.
        let (strat, source) = resolve_strategy(&s, "duckduckgo").unwrap();
        assert_eq!(source, "custom");
        assert_eq!(strat.version, "2099-01-01");
        // Reset drops it → back to built-in.
        let dropped = call_tool(
            &mut s,
            "web_search_reset_strategy",
            &json!({ "engine": "duckduckgo" }),
        )
        .await
        .unwrap();
        assert_eq!(dropped["dropped"], true);
        assert_eq!(resolve_strategy(&s, "duckduckgo").unwrap().1, "built-in");
    }

    // Loading an invalid strategy (missing the required html selectors) is rejected.
    #[tokio::test]
    async fn load_strategy_validates_required_keys() {
        let mut s = Session::new();
        let bad = call_tool(
            &mut s,
            "web_search_load_strategy",
            &json!({ "strategy": {
                "engine": "x",
                "query_url": "https://x/?q={query}",
                "format": "html"
            }}),
        )
        .await;
        assert!(bad.is_err(), "missing selectors should be rejected");
        // A query_url without {query} is rejected too.
        let bad2 = call_tool(
            &mut s,
            "web_search_load_strategy",
            &json!({ "strategy": {
                "engine": "x",
                "query_url": "https://x/search",
                "format": "json",
                "json_path": { "results": "r", "title": "t", "url": "u" }
            }}),
        )
        .await;
        assert!(
            bad2.is_err(),
            "query_url missing {{query}} should be rejected"
        );
    }

    // `web_search_strategies` lists all active strategies with their source; a session
    // override flips that engine's source to "custom".
    #[tokio::test]
    async fn search_strategies_lists_sources() {
        let mut s = Session::new();
        let list = call_tool(&mut s, "web_search_strategies", &json!({}))
            .await
            .unwrap();
        let arr = list.as_array().unwrap();
        let engines: Vec<&str> = arr.iter().map(|e| e["engine"].as_str().unwrap()).collect();
        for e in ["duckduckgo", "bing", "google", "searxng", "baidu"] {
            assert!(engines.contains(&e), "missing '{e}' in listing");
        }
        // All built-in by default.
        assert!(arr.iter().all(|e| e["source"] == "built-in"));
        // After an override, that engine reports "custom".
        call_tool(
            &mut s,
            "web_search_load_strategy",
            &json!({ "strategy": {
                "engine": "google",
                "version": "2099-01-01",
                "query_url": "https://x/?q={query}",
                "format": "html",
                "result_container": ".r", "title": "a", "link": "a"
            }}),
        )
        .await
        .unwrap();
        let list = call_tool(&mut s, "web_search_strategies", &json!({}))
            .await
            .unwrap();
        let google = list
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["engine"] == "google")
            .unwrap();
        assert_eq!(google["source"], "custom");
        assert_eq!(google["version"], "2099-01-01");
    }

    // `web_search_set_engine` stores a session default that `web_search` uses when a
    // call omits `engine`; an explicit `engine` arg still overrides it. Unknown → error.
    #[tokio::test]
    async fn set_engine_sets_session_default_and_arg_overrides() {
        let mut s = Session::new();
        // No default yet → falls through to duckduckgo.
        assert_eq!(pick_search_engine(&s, &json!({})), "duckduckgo");
        // Set a session default.
        let r = call_tool(
            &mut s,
            "web_search_set_engine",
            &json!({ "engine": "bing" }),
        )
        .await
        .unwrap();
        assert_eq!(r["ok"], true);
        assert_eq!(r["engine"], "bing");
        // A no-engine call now uses the stored default.
        assert_eq!(pick_search_engine(&s, &json!({})), "bing");
        // An explicit engine arg still wins.
        assert_eq!(
            pick_search_engine(&s, &json!({ "engine": "google" })),
            "google"
        );
        // Unknown engine → the registry's "unknown engine" error; default unchanged.
        let bad = call_tool(
            &mut s,
            "web_search_set_engine",
            &json!({ "engine": "bogus" }),
        )
        .await;
        assert!(bad.unwrap_err().contains("unknown engine 'bogus'"));
        assert_eq!(pick_search_engine(&s, &json!({})), "bing");
    }

    // The interpreter honors a runtime-loaded strategy end to end (parse only).
    #[test]
    fn interpreter_drives_a_loaded_strategy() {
        let g = strat(
            r#"{
              "engine":"g","version":"t",
              "query_url":"https://g/search?q={query}&num={limit}",
              "format":"html","result_container":"div.g",
              "title":"h3","link":"a[href]","link_redirect_param":"q","snippet":".VwiC3b"
            }"#,
        );
        assert_eq!(
            build_query_url(&g, "a b", 3, None).unwrap(),
            "https://g/search?q=a+b&num=3"
        );
        let r = parse_serp(&g, GOOGLE_HTML, 10);
        assert_eq!(r[0].title, "Google One");
        assert_eq!(r[0].url, "https://example.com/g1");
    }

    #[tokio::test]
    async fn run_playwright_script_over_loaded_page() {
        let mut s = Session::new();
        s.load(
            "https://x.test/",
            "<main><h1>Widget</h1><button data-test-id='go'>Add</button>\
             <input id='q' value='hi'><p class='d'>nice widget</p></main>",
        );
        // A Playwright-style script: locators + getByTestId(config) + expect, no browser.
        let r = call_tool(
            &mut s,
            "run_playwright",
            &json!({
                "testIdAttribute": "data-test-id",
                "script": "\
                    await expect(page.locator('h1')).toHaveText('Widget');\n\
                    await expect(page.getByTestId('go')).toHaveCount(1);\n\
                    await expect(page.locator('.d')).toContainText('widget');\n\
                    await page.fill('#q', 'rust');\n\
                    await expect(page.locator('#q')).toHaveValue('rust');\n\
                    await expect(page.locator('button')).not.toHaveCount(5);\n\
                    expect(2 + 2).toBe(4);"
            }),
        )
        .await
        .unwrap();
        assert_eq!(r["ok"], true, "script should pass: {r}");

        // A failing assertion surfaces ok:false + the message (not a hard error).
        let bad = call_tool(
            &mut s,
            "run_playwright",
            &json!({ "script": "await expect(page.locator('h1')).toHaveText('Nope');" }),
        )
        .await
        .unwrap();
        assert_eq!(bad["ok"], false, "{bad}");
        assert!(
            bad["error"].as_str().unwrap().contains("expected text"),
            "{bad}"
        );

        // test() blocks are collected + run.
        let suite = call_tool(
            &mut s,
            "run_playwright",
            &json!({ "script": "test('h1 ok', async ({ page, expect }) => { await expect(page.locator('h1')).toHaveText('Widget'); });" }),
        )
        .await
        .unwrap();
        assert_eq!(suite["ok"], true, "{suite}");
        assert_eq!(suite["ran"][0], "h1 ok", "{suite}");
    }
}
