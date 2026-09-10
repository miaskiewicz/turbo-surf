//! Optional **challenge-solver integration**: detect a JS-challenge / proof-of-work
//! WAF wall (Akamai Bot Manager, DataDome, Kasada, Cloudflare) and hand it to a
//! server-side solver (Hyper Solutions or Scrapfly) that runs the vendor's VM and
//! returns valid tokens/cookies to replay on turbo-surf's fast path.
//!
//! turbo-surf cannot self-solve these in-isolate (no raster/GPU for active
//! canvas/WebGL fingerprints; the VMs re-obfuscate continuously) — see the tier-3
//! notes. This module is the seam to *rent* that layer: detection is local and
//! free; [`ChallengeSolver`] is the pluggable remote step.
//!
//! Configuration is via env (optionally a `.env` file): [`solver_from_env`]
//! returns `None` when no key is set, so this is entirely inert until wired to a
//! real account. Placeholder keys (`.env.example`) are recognised and ignored.
//!
//! **IP/JA3 pinning:** solver-issued tokens are bound to the egress IP + TLS
//! fingerprint that generated them. Pass the same proxy via [`SolveContext`] that
//! you replay the token through, or the token is rejected (the #1 failure mode).

use crate::http_backend as http;
use std::time::Duration;

/// The anti-bot vendor behind a detected wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vendor {
    Akamai,
    DataDome,
    Kasada,
    Cloudflare,
    /// AWS WAF Bot Control (the bot layer behind CloudFront / ALB).
    AwsWaf,
    /// Google reCAPTCHA — the `/sorry` unusual-traffic wall or a `g-recaptcha`
    /// widget on the page. Solved in-isolate by [`crate::recaptcha::RecaptchaSolver`].
    // `#[default]` only so `Challenge` can derive `Default` — the MCP `solve_recaptcha`
    // tool builds a `Challenge` by filling its fields explicitly.
    #[default]
    Recaptcha,
}

impl Vendor {
    pub fn as_str(self) -> &'static str {
        match self {
            Vendor::Akamai => "akamai",
            Vendor::DataDome => "datadome",
            Vendor::Kasada => "kasada",
            Vendor::Cloudflare => "cloudflare",
            Vendor::AwsWaf => "awswaf",
            Vendor::Recaptcha => "recaptcha",
        }
    }
}

/// A detected challenge wall on a response.
#[derive(Debug, Clone, Default)]
pub struct Challenge {
    pub vendor: Vendor,
    pub page_url: String,
    /// reCAPTCHA site key lifted from the page (`data-sitekey`, the `render=` query
    /// param, or a `grecaptcha.render(...)` literal). `None` for the other vendors.
    pub sitekey: Option<String>,
    /// reCAPTCHA v3 `action` label, when the page declares one. `None` otherwise.
    pub action: Option<String>,
}

impl Challenge {
    /// Construct a bare challenge (no reCAPTCHA site key/action) — the shape the
    /// header/cookie-driven vendors use.
    pub fn new(vendor: Vendor, page_url: impl Into<String>) -> Self {
        Self {
            vendor,
            page_url: page_url.into(),
            sitekey: None,
            action: None,
        }
    }
}

/// Identity to pin the solve to — must match what you replay the token through.
#[derive(Debug, Clone, Default)]
pub struct SolveContext {
    pub user_agent: String,
    /// Egress proxy URL (e.g. `http://user:pass@host:port`). The token is bound to
    /// this IP; replay through the same one.
    pub proxy: Option<String>,
}

/// A solved token: cookies (and any extra headers) to attach to subsequent
/// requests, valid for roughly `ttl`.
#[derive(Debug, Clone)]
pub struct SolvedToken {
    pub cookies: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub ttl: Duration,
}

#[derive(Debug)]
pub enum SolveError {
    NotConfigured,
    Http(String),
    Parse(String),
    Unsupported(Vendor),
    /// A reCAPTCHA case that CANNOT be solved in-isolate — a **v2 visual image-grid
    /// challenge** (needs vision/human). Honest boundary: the client VM produces a
    /// token only for score-based / invisible flows; a "select all squares with…"
    /// grid is out of reach without a real solver (route to Scrapfly/Hyper instead).
    VisualChallenge,
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::NotConfigured => write!(f, "no solver configured"),
            SolveError::Http(e) => write!(f, "solver HTTP error: {e}"),
            SolveError::Parse(e) => write!(f, "solver response parse error: {e}"),
            SolveError::Unsupported(v) => write!(f, "solver does not support {}", v.as_str()),
            SolveError::VisualChallenge => write!(
                f,
                "reCAPTCHA v2 image-grid challenge needs vision/human — not solvable \
                 in-isolate (route to an external solver)"
            ),
        }
    }
}
impl std::error::Error for SolveError {}

/// A pluggable remote solver. Implemented by [`HyperSolver`] and [`ScrapflySolver`].
#[async_trait::async_trait]
pub trait ChallengeSolver: Send + Sync {
    async fn solve(&self, ch: &Challenge, ctx: &SolveContext) -> Result<SolvedToken, SolveError>;
    fn name(&self) -> &'static str;
}

/// Runs a challenge's *own* JavaScript and returns the answer it computes — the
/// proper way to clear a JS-compute wall (e.g. Cloudflare): execute the script
/// instead of reverse-engineering its math. Implemented by the render tier
/// (`turbo-surf-render`) over the V8 isolate; injected into a solver so
/// `turbo-surf-core` stays render-free (no circular dep).
pub trait PowEngine: Send + Sync {
    /// Evaluate `script` (the challenge's compute) and return the answer string it
    /// produces (e.g. the value it assigns to the answer field).
    fn compute(&self, script: &str) -> Result<String, String>;
}

/// Runs the **in-isolate reCAPTCHA token flow** and returns the client
/// `g-recaptcha-response` token. Implemented by the render tier
/// (`turbo-surf-render`), which loads the page's own reCAPTCHA integration in a V8
/// isolate — the main-frame VM runs, `grecaptcha.render()`/`execute()` resolve over
/// a bridged bframe iframe + parent↔bframe postMessage handshake — and extracts the
/// token. Injected into [`RecaptchaSolver`] so `turbo-surf-core` stays render-free.
#[async_trait::async_trait]
pub trait RecaptchaEngine: Send + Sync {
    /// Run `page_html` (served from `page_url`, carrying the reCAPTCHA widget +
    /// `api.js`) in the isolate, drive `grecaptcha.execute(sitekey, {action})`, and
    /// return the token it produces. `Ok("")` / `Err` when no token could be minted
    /// (e.g. the flow escalated to a visual challenge).
    async fn execute(
        &self,
        page_html: &str,
        page_url: &str,
        sitekey: &str,
        action: &str,
    ) -> Result<String, String>;
}

/// Try `primary`; on a solve error fall back to `fallback`. Lets an *experimental*
/// in-house solver lead while a robust solver (e.g. the browser sidecar) catches
/// what it can't yet clear. See the Akamai routing in [`solver_from_env`].
pub struct FallbackSolver {
    primary: Box<dyn ChallengeSolver>,
    fallback: Box<dyn ChallengeSolver>,
}

impl FallbackSolver {
    pub fn new(primary: Box<dyn ChallengeSolver>, fallback: Box<dyn ChallengeSolver>) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for FallbackSolver {
    fn name(&self) -> &'static str {
        "fallback"
    }
    async fn solve(&self, ch: &Challenge, ctx: &SolveContext) -> Result<SolvedToken, SolveError> {
        match self.primary.solve(ch, ctx).await {
            Ok(t) => Ok(t),
            Err(_) => self.fallback.solve(ch, ctx).await,
        }
    }
}

// ---- detection -------------------------------------------------------------

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn has_set_cookie(headers: &[(String, String)], needle: &str) -> bool {
    headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("set-cookie") && v.contains(needle))
}

/// Sniff a response for a known challenge wall. Header-first (most reliable),
/// with a few body markers as backup. Returns `None` for a normal page — note a
/// site can *use* a vendor without the current response *being* a challenge, so
/// detection keys on challenge-specific signals (e.g. Cloudflare's
/// `cf-mitigated: challenge`, not the ever-present `Server: cloudflare`).
pub fn detect(
    page_url: &str,
    _status: u16,
    headers: &[(String, String)],
    body: &str,
) -> Option<Challenge> {
    let here = |vendor| Some(Challenge::new(vendor, page_url));

    // Cloudflare managed challenge / Turnstile.
    if header(headers, "cf-mitigated").is_some_and(|v| v.contains("challenge"))
        || has_set_cookie(headers, "cf_clearance")
        || body.contains("__cf_chl")
        || body.contains("/cdn-cgi/challenge-platform/")
    {
        return here(Vendor::Cloudflare);
    }
    // DataDome.
    if has_set_cookie(headers, "datadome")
        || header(headers, "x-datadome").is_some()
        || header(headers, "x-datadome-cid").is_some()
        || body.contains("geo.captcha-delivery.com")
    {
        return here(Vendor::DataDome);
    }
    // Kasada.
    if header(headers, "x-kpsdk-ct").is_some()
        || header(headers, "x-kpsdk-st").is_some()
        || body.contains("KPSDK")
        || body.contains("/ips.js")
    {
        return here(Vendor::Kasada);
    }
    // Akamai Bot Manager (its sensor cookies).
    if has_set_cookie(headers, "_abck")
        || has_set_cookie(headers, "bm_sz")
        || has_set_cookie(headers, "ak_bmsc")
    {
        return here(Vendor::Akamai);
    }
    // AWS WAF Bot Control (CloudFront/ALB): the challenge/captcha action header, the
    // aws-waf-token cookie, or the awswaf challenge/captcha assets.
    if header(headers, "x-amzn-waf-action").is_some()
        || has_set_cookie(headers, "aws-waf-token")
        || body.contains("token.awswaf.com")
        || body.contains("captcha.awswaf.com")
        || body.contains("challenge.js")
    {
        return here(Vendor::AwsWaf);
    }
    // Google reCAPTCHA. Two shapes: the `/sorry` unusual-traffic interstitial, and a
    // `g-recaptcha` widget embedded on a page. Carry the site key/action so the solver
    // (or the MCP tool) can drive the token flow without re-parsing.
    let sorry_wall = page_url.contains("/sorry/")
        || body.contains("our systems have detected unusual traffic")
        || body.contains("unusual traffic from your computer network");
    let has_widget = body.contains("g-recaptcha")
        || body.contains("grecaptcha.render")
        || body.contains("grecaptcha.execute")
        || body.contains("/recaptcha/api.js")
        || body.contains("/recaptcha/enterprise.js");
    if sorry_wall || has_widget {
        let sitekey = crate::recaptcha::extract_sitekey(body);
        // A `/sorry` wall with no discoverable widget is still a reCAPTCHA wall; a bare
        // widget mention with no site key is not actionable, so require one there.
        if sorry_wall || sitekey.is_some() {
            return Some(Challenge {
                vendor: Vendor::Recaptcha,
                page_url: page_url.to_string(),
                sitekey,
                action: crate::recaptcha::extract_action(body),
            });
        }
    }
    None
}

// ---- orchestration ---------------------------------------------------------

/// Detect a wall on a response and, if present, solve it. `Ok(None)` means the
/// response was a normal page (no challenge).
pub async fn solve_if_challenged(
    solver: &dyn ChallengeSolver,
    page_url: &str,
    status: u16,
    headers: &[(String, String)],
    body: &str,
    ctx: &SolveContext,
) -> Result<Option<SolvedToken>, SolveError> {
    match detect(page_url, status, headers, body) {
        Some(ch) => solver.solve(&ch, ctx).await.map(Some),
        None => Ok(None),
    }
}

// Parse a `{ "cookies": {k:v}, "headers": {k:v} }` token envelope into a
// SolvedToken. Shared by the Hyper and browser-sidecar adapters — vendors return
// either ready-to-set cookies (Akamai `_abck`) or a header token (Kasada
// `x-kpsdk-ct`), so both shapes are accepted; an empty envelope is an error.
fn token_from_json(json: &serde_json::Value) -> Result<SolvedToken, SolveError> {
    let mut cookies = Vec::new();
    let mut headers = Vec::new();
    if let Some(obj) = json["cookies"].as_object() {
        for (k, v) in obj {
            if let Some(v) = v.as_str() {
                cookies.push((k.clone(), v.to_string()));
            }
        }
    }
    if let Some(obj) = json["headers"].as_object() {
        for (k, v) in obj {
            if let Some(v) = v.as_str() {
                headers.push((k.clone(), v.to_string()));
            }
        }
    }
    if cookies.is_empty() && headers.is_empty() {
        return Err(SolveError::Parse(format!("no token in response: {json}")));
    }
    Ok(SolvedToken {
        cookies,
        headers,
        // Akamai _abck / Kasada ct / cf_clearance are reusable for tens of minutes.
        ttl: Duration::from_secs(30 * 60),
    })
}

fn enc(s: &str) -> String {
    // Minimal percent-encoding for query values (RFC 3986 unreserved kept raw).
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---- Scrapfly adapter ------------------------------------------------------

/// Scrapfly's Anti-Scraping-Protection (ASP) API: it renders the page through a
/// real browser + residential proxy and returns the page plus the cleared
/// cookies. Base URL is overridable for tests.
pub struct ScrapflySolver {
    api_key: String,
    base_url: String,
    client: http::Client,
}

impl ScrapflySolver {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.scrapfly.io".to_string(),
            client: crate::net::build_client(),
        }
    }
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for ScrapflySolver {
    fn name(&self) -> &'static str {
        "scrapfly"
    }

    async fn solve(&self, ch: &Challenge, ctx: &SolveContext) -> Result<SolvedToken, SolveError> {
        // GET /scrape?key=..&url=..&asp=true&render_js=true[&proxy_pool=..]
        let mut url = format!(
            "{}/scrape?key={}&asp=true&render_js=true&url={}",
            self.base_url,
            enc(&self.api_key),
            enc(&ch.page_url),
        );
        if ctx.proxy.is_some() {
            url.push_str("&proxy_pool=public_residential_pool");
        }
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        let text = resp
            .text()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| SolveError::Parse(e.to_string()))?;
        // result.cookies: [{ name, value, ... }]
        let cookies = json["result"]["cookies"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        Some((
                            c["name"].as_str()?.to_string(),
                            c["value"].as_str()?.to_string(),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(SolvedToken {
            cookies,
            headers: Vec::new(),
            ttl: Duration::from_secs(30 * 60),
        })
    }
}

// ---- Hyper Solutions adapter ----------------------------------------------

/// Hyper Solutions (matched to `hyper-sdk-go`): a server-side **sensor generator**,
/// not a cookie service. We POST the challenge inputs to their Akamai endpoint
/// (`https://akm.hypersolutions.co/v2/sensor`, auth header `x-api-key`), get back
/// `{payload}` — the `sensor_data` string — and then POST *that* to the target's
/// own (dynamic) sensor endpoint, where the edge sets the real `_abck`.
///
/// Akamai is the verified lane. Other vendors use different Hyper hosts/shapes not
/// reproduced here yet → `Unsupported` so the caller can fall back. Base URL
/// overridable for tests.
pub struct HyperSolver {
    api_key: String,
    base_url: String,
    client: http::Client,
}

impl HyperSolver {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            // The Akamai sensor host (per hyper-sdk-go); overridable for tests.
            base_url: "https://akm.hypersolutions.co".to_string(),
            client: crate::net::build_client(),
        }
    }
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    // Ask Hyper for a sensor_data payload. Body + auth + response field per
    // hyper-sdk-go: x-api-key header, {abck,bmsz,version,pageUrl,userAgent,script,
    // acceptLanguage,ip}, response `{payload}`.
    async fn generate_sensor(
        &self,
        ch: &Challenge,
        ctx: &SolveContext,
    ) -> Result<String, SolveError> {
        let body = serde_json::json!({
            "abck": "",
            "bmsz": "",
            "version": "2",
            "pageUrl": ch.page_url,
            "userAgent": ctx.user_agent,
            "script": "",
            "acceptLanguage": "en-US,en;q=0.9",
            "ip": "",
        });
        let resp = self
            .client
            .post(format!("{}/v2/sensor", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        let text = resp
            .text()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| SolveError::Parse(e.to_string()))?;
        if let Some(err) = json["error"].as_str().filter(|e| !e.is_empty()) {
            return Err(SolveError::Http(format!("hyper: {err}")));
        }
        json["payload"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .ok_or_else(|| SolveError::Parse(format!("hyper: no payload in {text}")))
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for HyperSolver {
    fn name(&self) -> &'static str {
        "hyper"
    }

    async fn solve(&self, ch: &Challenge, ctx: &SolveContext) -> Result<SolvedToken, SolveError> {
        if ch.vendor != Vendor::Akamai {
            // Only the Akamai sensor lane is wired to the real API; let the caller
            // fall back for DataDome/Kasada/CF/AWS.
            return Err(SolveError::Unsupported(ch.vendor));
        }
        // 1. Hyper generates the sensor_data string.
        let sensor = self.generate_sensor(ch, ctx).await?;
        // 2. POST it to the target's sensor endpoint (the page URL); the edge sets
        //    the real _abck via Set-Cookie, which we surface as the solved token.
        let resp = self
            .client
            .post(&ch.page_url)
            .header("content-type", "application/json")
            .body(serde_json::json!({ "sensor_data": sensor }).to_string())
            .send()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        let cookies: Vec<(String, String)> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|line| {
                line.strip_prefix("_abck=").map(|rest| {
                    (
                        "_abck".to_string(),
                        rest.split(';').next().unwrap_or("").to_string(),
                    )
                })
            })
            .collect();
        if cookies.is_empty() {
            return Err(SolveError::Parse(
                "hyper: no _abck after sensor POST".into(),
            ));
        }
        Ok(SolvedToken {
            cookies,
            headers: Vec::new(),
            ttl: Duration::from_secs(30 * 60),
        })
    }
}

// ---- headless-browser sidecar adapter -------------------------------------

/// Solve via a **user-supplied hardened-headless browser sidecar** — Chromium
/// stays entirely out of this tree. `BrowserSolver` shells to a command (e.g. a
/// nodriver / patchright / Camoufox script) over a JSON contract: it writes a
/// request on the child's stdin and reads a token envelope on stdout.
///
/// Request (stdin):  `{ "url", "vendor", "userAgent", "proxy" }`
/// Response (stdout): `{ "cookies": {name: value}, "headers": {name: value} }`
///
/// The browser does the real PoW + canvas/WebGL raster a real GPU/V8 can satisfy
/// (Kasada, active-canvas DataDome); turbo-surf replays the cleared cookies on
/// the fast path. Opt-in only (`TURBO_SURF_SOLVER=browser` + `TURBO_SURF_BROWSER_CMD`)
/// — never auto-selected. Run the browser through the SAME proxy you replay
/// through, and let its Chrome version match the `impersonate` profile so the
/// token's IP/JA3 binding holds on replay.
pub struct BrowserSolver {
    cmd: String,
}

impl BrowserSolver {
    pub fn new(cmd: String) -> Self {
        Self { cmd }
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for BrowserSolver {
    fn name(&self) -> &'static str {
        "browser"
    }

    async fn solve(&self, ch: &Challenge, ctx: &SolveContext) -> Result<SolvedToken, SolveError> {
        use tokio::io::AsyncWriteExt;
        let req = serde_json::json!({
            "url": ch.page_url,
            "vendor": ch.vendor.as_str(),
            "userAgent": ctx.user_agent,
            "proxy": ctx.proxy,
        })
        .to_string();
        // `sh -c` so the configured value can be a full command line with args.
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&self.cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| SolveError::Http(format!("spawn sidecar: {e}")))?;
        // Best-effort: a sidecar that ignores stdin is fine (the write may EPIPE).
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(req.as_bytes()).await;
        } // stdin dropped here → EOF, so the child can finish
        let out = child
            .wait_with_output()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        if !out.status.success() {
            return Err(SolveError::Http(format!(
                "sidecar exit {:?}: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&out.stdout).map_err(|e| SolveError::Parse(e.to_string()))?;
        token_from_json(&json)
    }
}

// ---- env / .env configuration ---------------------------------------------

// Treat the committed `.env.example` placeholders as "unset".
fn is_real_key(v: &str) -> bool {
    !v.is_empty() && !v.starts_with("your_") && !v.contains("_here")
}

// Load `.env` (first found walking up from CWD) into the process env, without
// overwriting already-set vars. Tiny parser — no dotenv dependency.
fn load_dotenv() {
    for candidate in [".env", "../.env", "../../.env", "../../../.env"] {
        let Ok(contents) = std::fs::read_to_string(candidate) else {
            continue;
        };
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim().trim_matches('"'));
                if std::env::var(k).is_err() {
                    // Safe on edition 2021; this runs before any solver thread.
                    std::env::set_var(k, v);
                }
            }
        }
        return;
    }
}

/// Build a solver from env (loading `.env` if present). `TURBO_SURF_SOLVER` picks
/// `hyper`|`scrapfly`; otherwise the first key present wins. Returns `None` when
/// nothing is configured — so the whole feature is inert until a real key is set.
pub fn solver_from_env() -> Option<Box<dyn ChallengeSolver>> {
    solver_from_env_pow(None, None)
}

// Wrap an in-house solver so a failed solve falls back to the browser sidecar when
// `TURBO_SURF_BROWSER_CMD` is set; otherwise return it unwrapped.
fn maybe_browser_fallback(primary: Box<dyn ChallengeSolver>) -> Box<dyn ChallengeSolver> {
    match std::env::var("TURBO_SURF_BROWSER_CMD")
        .ok()
        .filter(|c| !c.trim().is_empty())
    {
        Some(cmd) => Box::new(FallbackSolver::new(
            primary,
            Box::new(BrowserSolver::new(cmd)),
        )),
        None => primary,
    }
}

/// Like [`solver_from_env`] but with the render-tier engines injected — a
/// [`PowEngine`] (Cloudflare/AWS run the challenge's own JS to compute the answer,
/// the proper path) and a [`RecaptchaEngine`] (`TURBO_SURF_SOLVER=recaptcha` drives
/// the in-isolate token flow). The render tier passes V8 engines here; a solver that
/// doesn't need one ignores it.
pub fn solver_from_env_pow(
    pow: Option<Box<dyn PowEngine>>,
    recaptcha: Option<Box<dyn RecaptchaEngine>>,
) -> Option<Box<dyn ChallengeSolver>> {
    load_dotenv();
    let hyper = std::env::var("HYPER_API_KEY")
        .ok()
        .filter(|v| is_real_key(v));
    let scrapfly = std::env::var("SCRAPFLY_API_KEY")
        .ok()
        .filter(|v| is_real_key(v));
    let pick = std::env::var("TURBO_SURF_SOLVER")
        .unwrap_or_default()
        .to_ascii_lowercase();
    match pick.as_str() {
        "hyper" => hyper.map(|k| Box::new(HyperSolver::new(k)) as Box<dyn ChallengeSolver>),
        "scrapfly" => {
            scrapfly.map(|k| Box::new(ScrapflySolver::new(k)) as Box<dyn ChallengeSolver>)
        }
        // The headless-browser sidecar is opt-in ONLY: it spawns an external
        // process (GPU/proxy cost), so it never joins the auto fallback below —
        // `TURBO_SURF_SOLVER=browser` must select it explicitly.
        "browser" => std::env::var("TURBO_SURF_BROWSER_CMD")
            .ok()
            .filter(|c| !c.trim().is_empty())
            .map(|cmd| Box::new(BrowserSolver::new(cmd)) as Box<dyn ChallengeSolver>),
        // In-house solvers try themselves first; when a browser sidecar is
        // configured (TURBO_SURF_BROWSER_CMD), a failed in-house solve falls back to
        // the real Chromium so the wall still clears. Browserless → in-house only.
        // Akamai is EXPERIMENTAL (live sensor encoding not reversed per-version).
        "akamai" => Some(maybe_browser_fallback(Box::new(
            crate::akamai::AkamaiSolver::new(),
        ))),
        "cloudflare" => {
            let mut cf = crate::cloudflare::CloudflareSolver::new();
            if let Some(engine) = pow {
                cf = cf.with_pow_engine(engine);
            }
            Some(maybe_browser_fallback(Box::new(cf)))
        }
        "awswaf" | "aws" => {
            let mut waf = crate::aws_waf::AwsWafSolver::new();
            if let Some(engine) = pow {
                waf = waf.with_pow_engine(engine);
            }
            Some(maybe_browser_fallback(Box::new(waf)))
        }
        // reCAPTCHA runs the token flow in the V8 render tier (the RecaptchaEngine).
        // When a browser sidecar is also configured, a failed in-isolate solve (e.g.
        // a v2 image challenge) falls back to the real browser.
        "recaptcha" => {
            let mut solver = crate::recaptcha::RecaptchaSolver::new();
            if let Some(engine) = recaptcha {
                solver = solver.with_engine(engine);
            }
            Some(maybe_browser_fallback(Box::new(solver)))
        }
        _ => hyper
            .map(|k| Box::new(HyperSolver::new(k)) as Box<dyn ChallengeSolver>)
            .or_else(|| {
                scrapfly.map(|k| Box::new(ScrapflySolver::new(k)) as Box<dyn ChallengeSolver>)
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The env-mutating tests share process env; serialize them.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn hdr(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn detects_each_vendor() {
        let u = "https://x.test/";
        assert_eq!(
            detect(u, 403, &hdr(&[("set-cookie", "_abck=1~-1~...")]), "").map(|c| c.vendor),
            Some(Vendor::Akamai)
        );
        assert_eq!(
            detect(u, 403, &hdr(&[("set-cookie", "datadome=abc; Path=/")]), "").map(|c| c.vendor),
            Some(Vendor::DataDome)
        );
        assert_eq!(
            detect(u, 429, &hdr(&[("x-kpsdk-ct", "tok")]), "").map(|c| c.vendor),
            Some(Vendor::Kasada)
        );
        assert_eq!(
            detect(u, 403, &hdr(&[("cf-mitigated", "challenge")]), "").map(|c| c.vendor),
            Some(Vendor::Cloudflare)
        );
        assert_eq!(
            detect(u, 202, &hdr(&[("x-amzn-waf-action", "challenge")]), "").map(|c| c.vendor),
            Some(Vendor::AwsWaf)
        );
        // A plain page is not a challenge.
        assert!(detect(u, 200, &hdr(&[("server", "cloudflare")]), "<html>ok</html>").is_none());
    }

    #[test]
    fn body_markers_detect() {
        let u = "https://x.test/";
        assert_eq!(
            detect(u, 200, &[], "loading <script src=\"/ips.js\">").map(|c| c.vendor),
            Some(Vendor::Kasada)
        );
        assert_eq!(
            detect(u, 200, &[], "<div id=\"/cdn-cgi/challenge-platform/h/b\">").map(|c| c.vendor),
            Some(Vendor::Cloudflare)
        );
    }

    #[test]
    fn detects_recaptcha_sorry_and_widget() {
        // The google `/sorry` unusual-traffic wall (URL marker), with its embedded widget.
        let sorry = detect(
            "https://www.google.com/sorry/index?continue=https://x",
            429,
            &[],
            r#"<div>Our systems have detected unusual traffic</div>
               <div class="g-recaptcha" data-sitekey="6Lc_sorry_key"></div>"#,
        )
        .expect("/sorry is a reCAPTCHA wall");
        assert_eq!(sorry.vendor, Vendor::Recaptcha);
        assert_eq!(sorry.sitekey.as_deref(), Some("6Lc_sorry_key"));

        // The unusual-traffic body phrase alone (no /sorry path) also trips it.
        assert_eq!(
            detect(
                "https://x.test/",
                200,
                &[],
                "our systems have detected unusual traffic from your computer network"
            )
            .map(|c| c.vendor),
            Some(Vendor::Recaptcha)
        );

        // A bare `g-recaptcha` widget on an ordinary page — a solvable v3 site key.
        let widget = detect(
            "https://shop.test/checkout",
            200,
            &[],
            r#"<script src="https://www.google.com/recaptcha/api.js?render=6Lc_v3"></script>"#,
        )
        .expect("a reCAPTCHA widget is actionable");
        assert_eq!(widget.vendor, Vendor::Recaptcha);
        assert_eq!(widget.sitekey.as_deref(), Some("6Lc_v3"));

        // A plain page (no wall, no widget) is not a challenge.
        assert!(detect("https://x.test/", 200, &[], "<html>hello</html>").is_none());
    }

    #[test]
    fn env_selection_ignores_placeholders() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Fresh keys for this test; placeholders must read as unset.
        std::env::set_var("SCRAPFLY_API_KEY", "your_scrapfly_key_here");
        std::env::remove_var("HYPER_API_KEY");
        std::env::set_var("TURBO_SURF_SOLVER", "scrapfly");
        assert!(
            solver_from_env().is_none(),
            "placeholder counted as configured"
        );

        std::env::set_var("SCRAPFLY_API_KEY", "sk_real_abc123");
        let s = solver_from_env().expect("real key → solver");
        assert_eq!(s.name(), "scrapfly");
        std::env::remove_var("SCRAPFLY_API_KEY");
        std::env::remove_var("TURBO_SURF_SOLVER");
    }

    #[tokio::test]
    async fn browser_sidecar_returns_token() {
        // Fake hardened-headless sidecar: drain stdin, emit a token envelope.
        let solver = BrowserSolver::new(
            "cat >/dev/null; printf '{\"cookies\":{\"datadome\":\"DD\"},\"headers\":{\"x-kpsdk-ct\":\"K\"}}'"
                .into(),
        );
        let ch = Challenge::new(Vendor::DataDome, "https://x.test/");
        let token = solver.solve(&ch, &SolveContext::default()).await.unwrap();
        assert_eq!(
            token.cookies,
            vec![("datadome".to_string(), "DD".to_string())]
        );
        assert_eq!(
            token.headers,
            vec![("x-kpsdk-ct".to_string(), "K".to_string())]
        );
    }

    #[test]
    fn browser_solver_is_opt_in_only() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A browser command present but TURBO_SURF_SOLVER unset → NOT auto-picked
        // (the sidecar spawns a process; it must be chosen explicitly).
        std::env::set_var("TURBO_SURF_BROWSER_CMD", "my-headless-solver");
        std::env::remove_var("HYPER_API_KEY");
        std::env::remove_var("SCRAPFLY_API_KEY");
        std::env::remove_var("TURBO_SURF_SOLVER");
        assert!(solver_from_env().is_none(), "browser auto-selected");

        std::env::set_var("TURBO_SURF_SOLVER", "browser");
        assert_eq!(solver_from_env().map(|s| s.name()), Some("browser"));
        std::env::remove_var("TURBO_SURF_BROWSER_CMD");
        std::env::remove_var("TURBO_SURF_SOLVER");
    }

    #[tokio::test]
    async fn scrapfly_parses_cleared_cookies() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        // Mock the Scrapfly API: return its JSON envelope with cleared cookies.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf).await;
            let body = r#"{"result":{"cookies":[{"name":"cf_clearance","value":"TOKEN123"}]}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.flush().await;
        });
        let solver =
            ScrapflySolver::new("k".into()).with_base_url(format!("http://127.0.0.1:{port}"));
        let ch = Challenge::new(Vendor::Cloudflare, "https://x.test/");
        let token = solver.solve(&ch, &SolveContext::default()).await.unwrap();
        assert_eq!(
            token.cookies,
            vec![("cf_clearance".to_string(), "TOKEN123".to_string())]
        );
    }

    // Hyper's real two-step Akamai flow (per hyper-sdk-go): POST inputs to
    // /v2/sensor (x-api-key auth) → `{payload}` → POST that as sensor_data to the
    // target → edge sets `_abck`. One mock server plays both roles by path.
    #[tokio::test]
    async fn hyper_generates_sensor_then_harvests_abck() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let resp = if req.contains("/v2/sensor") {
                        // The sensor generator: require the real auth header, return payload.
                        assert!(req.contains("x-api-key:"), "missing x-api-key: {req}");
                        let b = r#"{"payload":"SENSOR_DATA_STR","context":"ctx"}"#;
                        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", b.len(), b)
                    } else {
                        // The target sensor endpoint: must receive the generated sensor_data.
                        assert!(req.contains("SENSOR_DATA_STR"), "sensor not posted: {req}");
                        let b = "{}";
                        format!("HTTP/1.1 200 OK\r\nSet-Cookie: _abck=REAL~0~ok; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", b.len(), b)
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        let base = format!("http://127.0.0.1:{port}");
        // Target page URL = same mock (non-/v2/sensor path).
        let solver = HyperSolver::new("KEY".into()).with_base_url(base.clone());
        let ch = Challenge::new(Vendor::Akamai, format!("{base}/target"));
        let token = solver.solve(&ch, &SolveContext::default()).await.unwrap();
        let abck = token.cookies.iter().find(|(k, _)| k == "_abck").unwrap();
        assert!(abck.1.starts_with("REAL"), "expected harvested _abck");
        // Non-Akamai vendors fall through as Unsupported.
        let cf = Challenge::new(Vendor::Cloudflare, base);
        assert!(matches!(
            solver.solve(&cf, &SolveContext::default()).await,
            Err(SolveError::Unsupported(Vendor::Cloudflare))
        ));
    }
}
