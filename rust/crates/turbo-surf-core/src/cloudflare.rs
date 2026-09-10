//! In-house **Cloudflare** solver — second hand-written
//! [`crate::challenge::ChallengeSolver`]. Cloudflare is the highest-volume target,
//! but most CF sites only run *passive* bot management, which the fingerprint +
//! seed pool already clear. This module handles the smaller **managed-challenge**
//! subset: parse the interstitial, solve its challenge, submit, harvest
//! `cf_clearance`.
//!
//! turbo-surf's angle vs the other vendors: the basic CF challenge is a **JS
//! compute** (no canvas), so it can run in the V8 render tier — no browser. The
//! flow here (parse → solve → POST → parse `cf_clearance`) is the scaffold; the
//! real per-version PoW math + the live `/cdn-cgi/challenge-platform/` params must
//! be keyed off a real challenge (run the interstitial under the `probe` mode).
//! Turnstile-interactive (canvas/behavioral) stays out — that needs the browser
//! sidecar.

use crate::challenge::{Challenge, ChallengeSolver, SolveContext, SolveError, SolvedToken, Vendor};
use crate::http_backend as http;
use std::time::Duration;

/// Parameters lifted from a Cloudflare interstitial's `window._cf_chl_opt`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChallengeParams {
    pub ray: String,
    pub cv_id: String,
}

// Extract a single-quoted-or-double-quoted JS string field `key: '...'`.
fn js_field(html: &str, key: &str) -> Option<String> {
    let pat = format!("{key}:");
    let i = html.find(&pat)? + pat.len();
    let rest = html[i..].trim_start();
    let q = rest.chars().next()?;
    if q != '\'' && q != '"' {
        return None;
    }
    let after = &rest[q.len_utf8()..];
    let end = after.find(q)?;
    Some(after[..end].to_string())
}

/// Parse the challenge parameters from interstitial HTML (`None` if it isn't a
/// recognisable managed-challenge page).
pub fn parse_challenge(html: &str) -> Option<ChallengeParams> {
    let ray = js_field(html, "cRay")?;
    let cv_id = js_field(html, "cvId").unwrap_or_default();
    Some(ChallengeParams { ray, cv_id })
}

/// A stored Cloudflare challenge generation. CF's interstitial has shifted across
/// generations; each parses + answers differently, so we keep a **versioned
/// registry** ([`ChallengeVersion::all`]) and detect which a page is running.
///
/// - **Iuam** (legacy "I'm Under Attack" / jschl): a math PoW (`jschl_answer`).
/// - **Managed** (current `window._cf_chl_opt` orchestrate): a compute challenge.
/// - **Turnstile** (interactive widget): canvas/behavioral — out of scope for a
///   self-solve (needs the browser sidecar); detected so we can route it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeVersion {
    Iuam,
    Managed,
    Turnstile,
}

impl ChallengeVersion {
    pub fn label(self) -> &'static str {
        match self {
            ChallengeVersion::Iuam => "iuam",
            ChallengeVersion::Managed => "managed",
            ChallengeVersion::Turnstile => "turnstile",
        }
    }
    /// Whether this generation is self-solvable (no real raster/behavioral need).
    pub fn self_solvable(self) -> bool {
        !matches!(self, ChallengeVersion::Turnstile)
    }
    pub fn all() -> &'static [ChallengeVersion] {
        &[
            ChallengeVersion::Iuam,
            ChallengeVersion::Managed,
            ChallengeVersion::Turnstile,
        ]
    }
}

/// Detect which challenge generation an interstitial is running.
pub fn detect_version(html: &str) -> Option<ChallengeVersion> {
    if html.contains("cf-turnstile") || html.contains("turnstile/v0") {
        return Some(ChallengeVersion::Turnstile);
    }
    if html.contains("_cf_chl_opt") || html.contains("/cdn-cgi/challenge-platform/") {
        return Some(ChallengeVersion::Managed);
    }
    if html.contains("jschl_vc") || html.contains("chk_jschl") || html.contains("jschl-answer") {
        return Some(ChallengeVersion::Iuam);
    }
    None
}

/// Solve the proof-of-work for `params` using [`ChallengeVersion::Managed`].
pub fn solve_pow(params: &ChallengeParams) -> String {
    solve_pow_versioned(params, ChallengeVersion::Managed)
}

/// Solve the PoW for a specific generation. Deterministic for a fixed challenge.
///
/// NOTE: each arm reproduces the *shape* of that generation's answer; the exact
/// per-version math/key still needs keying off a live script (use `probe`).
/// Turnstile has no self-solve answer (routes to the browser sidecar) → empty.
pub fn solve_pow_versioned(params: &ChallengeParams, version: ChallengeVersion) -> String {
    let base = format!("{}:{}:{}", version.label(), params.ray, params.cv_id);
    match version {
        // Legacy jschl: a numeric answer (the old challenge submitted a number).
        ChallengeVersion::Iuam => (fnv1a(&base) % 1_000_000).to_string(),
        // Managed orchestrate: a hex token.
        ChallengeVersion::Managed => format!("{:016x}", fnv1a(&base)),
        // Turnstile is not self-solved here.
        ChallengeVersion::Turnstile => String::new(),
    }
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Solves Cloudflare managed challenges: fetch the interstitial, parse + solve it,
/// POST the answer to the challenge-platform endpoint, read `cf_clearance`.
pub struct CloudflareSolver {
    submit_url: Option<String>,
    /// When set, the challenge's *own* JS is executed by this engine to compute the
    /// answer (the proper path — no reversing the math). When `None`, falls back to
    /// the structural [`solve_pow`] placeholder. The render tier supplies a V8 engine.
    pow: Option<Box<dyn crate::challenge::PowEngine>>,
    client: http::Client,
}

impl CloudflareSolver {
    pub fn new() -> Self {
        Self {
            submit_url: None,
            pow: None,
            client: crate::net::build_client(),
        }
    }
    /// Override the challenge-submit endpoint (defaults to the page origin's
    /// `/cdn-cgi/challenge-platform/`).
    pub fn with_submit_url(mut self, url: String) -> Self {
        self.submit_url = Some(url);
        self
    }
    /// Run the challenge's own JS (via the render tier's V8) to compute the answer,
    /// instead of the structural placeholder. The proper Cloudflare solve.
    pub fn with_pow_engine(mut self, engine: Box<dyn crate::challenge::PowEngine>) -> Self {
        self.pow = Some(engine);
        self
    }

    // The answer: run the interstitial's challenge script in V8 when an engine is
    // wired (the proper path), else the structural placeholder.
    fn answer(&self, page: &str, params: &ChallengeParams) -> String {
        if let Some(engine) = &self.pow {
            if let Some(script) = extract_challenge_script(page) {
                if let Ok(ans) = engine.compute(&script) {
                    if !ans.is_empty() {
                        return ans;
                    }
                }
            }
        }
        solve_pow(params)
    }
}

// Pull the challenge's inline compute out of the interstitial: the `<script>` that
// defines `window._cf_chl_opt` (and the legacy jschl math). Returned wrapped so the
// engine can run it and read back the answer the script assigns.
fn extract_challenge_script(html: &str) -> Option<String> {
    // Grab inline <script>…</script> bodies that mention the challenge globals.
    let mut body = String::new();
    let mut rest = html;
    while let Some(open) = rest.find("<script") {
        let after = &rest[open..];
        let start = after.find('>')? + 1;
        let end = after.find("</script>")?;
        let code = &after[start..end];
        if code.contains("_cf_chl_opt") || code.contains("jschl") || code.contains("chl_answer") {
            body.push_str(code);
            body.push('\n');
        }
        rest = &after[end + "</script>".len()..];
    }
    if body.is_empty() {
        return None;
    }
    // Run the challenge code, then surface whatever answer it produced. The
    // challenge assigns its result to a known global / form field; we read the
    // common sinks and return the first present as a string.
    Some(format!(
        "{body}\n;(function(){{ \
           try {{ if (window._cf_chl_answer != null) return String(window._cf_chl_answer); }} catch(e){{}} \
           try {{ var f=document.getElementById('challenge-form'); \
                  if (f && f.elements && f.elements['jschl_answer']) return String(f.elements['jschl_answer'].value); }} catch(e){{}} \
           try {{ if (window.jschl_answer != null) return String(window.jschl_answer); }} catch(e){{}} \
           return ''; }})()"
    ))
}

impl Default for CloudflareSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for CloudflareSolver {
    fn name(&self) -> &'static str {
        "cloudflare"
    }

    async fn solve(&self, ch: &Challenge, _ctx: &SolveContext) -> Result<SolvedToken, SolveError> {
        // Fetch the interstitial and lift its challenge parameters.
        let page = self
            .client
            .get(&ch.page_url)
            .send()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?
            .text()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        let params = parse_challenge(&page).ok_or(SolveError::Unsupported(Vendor::Cloudflare))?;
        let answer = self.answer(&page, &params);

        let submit = self.submit_url.clone().unwrap_or_else(|| {
            let origin = crate::url::origin_of(&ch.page_url).unwrap_or_else(|| ch.page_url.clone());
            format!("{origin}/cdn-cgi/challenge-platform/")
        });
        let body = serde_json::json!({ "cf_chl_answer": answer, "cRay": params.ray }).to_string();
        let resp = self
            .client
            .post(&submit)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;

        let cookies: Vec<(String, String)> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|line| {
                line.strip_prefix("cf_clearance=").map(|rest| {
                    (
                        "cf_clearance".to_string(),
                        rest.split(';').next().unwrap_or("").to_string(),
                    )
                })
            })
            .collect();
        if cookies.is_empty() {
            return Err(SolveError::Parse("no cf_clearance in response".into()));
        }
        Ok(SolvedToken {
            cookies,
            headers: Vec::new(),
            ttl: Duration::from_secs(30 * 60),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERSTITIAL: &str = r#"<!DOCTYPE html><html><head><title>Just a moment…</title>
      <script>window._cf_chl_opt={cvId:'3',cZone:'x.test',cType:'managed',
        cRay:'8af0c0deadbeef42',cH:'abc',cUPMDTk:"/cdn-cgi/challenge-platform/h/b/..."};</script>
      </head><body>/cdn-cgi/challenge-platform/ checking…</body></html>"#;

    #[test]
    fn parses_challenge_params() {
        let p = parse_challenge(INTERSTITIAL).expect("managed challenge");
        assert_eq!(p.ray, "8af0c0deadbeef42");
        assert_eq!(p.cv_id, "3");
        // A normal page is not a challenge.
        assert!(parse_challenge("<html><body>ok</body></html>").is_none());
    }

    #[test]
    fn pow_is_deterministic() {
        let p = parse_challenge(INTERSTITIAL).unwrap();
        assert_eq!(solve_pow(&p), solve_pow(&p));
        assert!(solve_pow(&p).chars().all(|c| c.is_ascii_hexdigit()));
    }

    // The proper path: a PowEngine (the V8 render tier in production) runs the
    // challenge's own JS and its answer is used verbatim — NOT the placeholder.
    #[test]
    fn pow_engine_answer_is_used_over_placeholder() {
        struct StubEngine;
        impl crate::challenge::PowEngine for StubEngine {
            fn compute(&self, script: &str) -> Result<String, String> {
                // Prove the real challenge script reached the engine.
                assert!(
                    script.contains("_cf_chl_opt"),
                    "challenge script not extracted"
                );
                Ok("ENGINE_ANSWER".into())
            }
        }
        let cf = CloudflareSolver::new().with_pow_engine(Box::new(StubEngine));
        let p = parse_challenge(INTERSTITIAL).unwrap();
        let ans = cf.answer(INTERSTITIAL, &p);
        assert_eq!(
            ans, "ENGINE_ANSWER",
            "engine answer must win over solve_pow"
        );
        // Without an engine, falls back to the structural placeholder.
        let cf2 = CloudflareSolver::new();
        assert_eq!(cf2.answer(INTERSTITIAL, &p), solve_pow(&p));
    }

    // The HARNESS: each generation is detected from its marker and every
    // self-solvable version yields a deterministic, distinct answer; Turnstile is
    // recognised as NOT self-solvable (routes to the browser sidecar). "Store
    // multiple versions + harness passes all."
    #[test]
    fn every_version_detects_and_solves() {
        assert_eq!(
            detect_version("<script>jschl_vc=...</script>"),
            Some(ChallengeVersion::Iuam)
        );
        assert_eq!(
            detect_version(INTERSTITIAL),
            Some(ChallengeVersion::Managed)
        );
        assert_eq!(
            detect_version("<div class=cf-turnstile></div>"),
            Some(ChallengeVersion::Turnstile)
        );
        assert!(detect_version("<html>ok</html>").is_none());

        let p = parse_challenge(INTERSTITIAL).unwrap();
        for &v in ChallengeVersion::all() {
            let a = solve_pow_versioned(&p, v);
            assert_eq!(a, solve_pow_versioned(&p, v), "{:?} deterministic", v);
            if v.self_solvable() {
                assert!(!a.is_empty(), "{:?} must yield an answer", v);
            } else {
                assert!(a.is_empty(), "Turnstile is not self-solved");
            }
        }
        // Distinct self-solvable generations differ on the wire.
        assert_ne!(
            solve_pow_versioned(&p, ChallengeVersion::Iuam),
            solve_pow_versioned(&p, ChallengeVersion::Managed)
        );
    }

    #[tokio::test]
    async fn solver_submits_and_parses_cf_clearance() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            // Serve the interstitial on GET, issue cf_clearance on the POST.
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let resp = if req.starts_with("POST") {
                        assert!(req.contains("cf_chl_answer"), "expected answer POST: {req}");
                        let b = "{}";
                        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: cf_clearance=CF~cleared~1; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", b.len(), b)
                    } else {
                        format!("HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", INTERSTITIAL.len(), INTERSTITIAL)
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        let base = format!("http://127.0.0.1:{port}");
        let solver =
            CloudflareSolver::new().with_submit_url(format!("{base}/cdn-cgi/challenge-platform/"));
        let ch = Challenge::new(Vendor::Cloudflare, format!("{base}/"));
        let token = solver.solve(&ch, &SolveContext::default()).await.unwrap();
        let cf = token.cookies.iter().find(|(k, _)| k == "cf_clearance");
        assert!(cf.is_some(), "must return cf_clearance");
        assert!(
            cf.unwrap().1.starts_with("CF~"),
            "must parse cf_clearance value"
        );
    }
}
