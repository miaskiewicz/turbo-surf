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
use std::collections::BTreeMap;
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

pub const VERSION: &str = "0.3.2";

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
        parts.join("\n;\n")
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
            self.fetch_page_images(&html).await
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
    async fn fetch_page_images(&mut self, html: &str) -> raster::ImageAssets {
        const MAX_IMAGES: usize = 60;
        let base = self.url.clone();
        let mut assets = raster::ImageAssets::new();
        for name in raster::image_urls(html).into_iter().take(MAX_IMAGES) {
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
