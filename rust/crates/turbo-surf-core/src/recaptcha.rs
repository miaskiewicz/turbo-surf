//! In-house **Google reCAPTCHA** solver — a [`crate::challenge::ChallengeSolver`]
//! driven by the render tier's *in-isolate* reCAPTCHA token flow. turbo-surf loads
//! the page's own reCAPTCHA integration in a true V8 isolate: the main-frame VM
//! runs, `grecaptcha.render()`/`execute()` resolve over a bridged bframe iframe +
//! parent↔bframe postMessage handshake (all fetched over the net stack), and the
//! client `g-recaptcha-response` token is read back — no browser.
//!
//! ## Honest scope — what this clears, and what it does NOT
//!
//! The token is minted by the **client VM only**. reCAPTCHA acceptance is scored
//! *server-side* by Google on behavioral + network signals (chiefly the egress IP).
//! So realistically this solves:
//!
//! - **reCAPTCHA v3 / invisible / score-based** flows (no visual challenge) — a
//!   token is produced and is accepted when the score is decent.
//! - the google **`/sorry`** unusual-traffic wall — but only when the score is good,
//!   i.e. paired with a clean egress IP (set `TURBO_SURF_PROXY`). A flagged IP is
//!   rejected no matter how valid the token is.
//!
//! It does **NOT** solve **reCAPTCHA v2 with a visual image-grid challenge** ("select
//! all squares with…") — that needs vision/a human and is out of reach in-isolate.
//! [`RecaptchaSolver`] detects that case and returns [`SolveError::VisualChallenge`]
//! so the caller can route it to an external solver (Scrapfly / Hyper) that does
//! solve challenges, or to the browser sidecar.

use crate::challenge::{
    Challenge, ChallengeSolver, RecaptchaEngine, SolveContext, SolveError, SolvedToken,
};
use crate::http_backend as http;
use std::time::Duration;

/// The kind of reCAPTCHA integration a page carries — decides whether the in-isolate
/// flow can realistically mint a usable token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecaptchaKind {
    /// v3 (programmatic `execute` with an `action`) — score-based, no visual step.
    V3,
    /// invisible v2 — a token unless the score forces a challenge.
    InvisibleV2,
    /// v2 checkbox widget — the case that pops an image grid; not self-solvable.
    CheckboxV2,
}

/// Extract a reCAPTCHA site key from page HTML: the widget's `data-sitekey`, the
/// `render=` query param on `api.js`/`enterprise.js`, a `grecaptcha.render(…, {sitekey})`
/// literal, or a `grecaptcha.execute('KEY', …)` first argument.
pub fn extract_sitekey(html: &str) -> Option<String> {
    quoted_after(html, "data-sitekey")
        .or_else(|| render_param(html))
        .or_else(|| quoted_after(html, "sitekey"))
        .or_else(|| execute_first_arg(html))
}

/// Extract a reCAPTCHA v3 `action` label (`data-action` attribute or an
/// `{action: '…'}` option), if the page declares one.
pub fn extract_action(html: &str) -> Option<String> {
    quoted_after(html, "data-action").or_else(|| quoted_after(html, "action"))
}

/// Classify the reCAPTCHA integration on a page. `render=`/programmatic → [`RecaptchaKind::V3`];
/// an explicit `size=invisible` → [`RecaptchaKind::InvisibleV2`]; a plain widget →
/// [`RecaptchaKind::CheckboxV2`] (the visual case).
pub fn classify(html: &str) -> RecaptchaKind {
    // A `render=<sitekey>` param (not the `explicit`/`onload` sentinels) means the page
    // drives `grecaptcha.execute` itself → v3 score flow.
    if render_param(html).is_some() {
        return RecaptchaKind::V3;
    }
    if quoted_after(html, "data-size")
        .map(|v| v.eq_ignore_ascii_case("invisible"))
        .unwrap_or(false)
        || quoted_after(html, "size")
            .map(|v| v.eq_ignore_ascii_case("invisible"))
            .unwrap_or(false)
    {
        return RecaptchaKind::InvisibleV2;
    }
    RecaptchaKind::CheckboxV2
}

/// Whether the page's reCAPTCHA is a **v2 visual image-grid** case — the one that
/// cannot be solved in-isolate (returns [`SolveError::VisualChallenge`]).
pub fn is_visual_v2_challenge(html: &str) -> bool {
    // Only when there IS a reCAPTCHA widget at all, and it's the checkbox kind. A page
    // with no widget (e.g. a bare `/sorry` shell) is left for the engine to attempt.
    (html.contains("g-recaptcha") || extract_sitekey(html).is_some())
        && classify(html) == RecaptchaKind::CheckboxV2
}

// --- parsing helpers --------------------------------------------------------

// Is `name` at `idx` a standalone token (not the tail of a longer identifier like
// `transaction` for `action`)? Requires the preceding char to be a non-identifier.
fn is_word_start(html: &str, idx: usize) -> bool {
    html[..idx]
        .chars()
        .next_back()
        .map(|c| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(true)
}

// Value of `name = "…"` / `name: '…'` (HTML attribute or JS object field). Tolerates a
// closing quote on a quoted key (`'sitekey': "…"`) and either `=` or `:` as separator.
fn quoted_after(html: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = html[from..].find(name) {
        let idx = from + i;
        from = idx + name.len();
        if !is_word_start(html, idx) {
            continue;
        }
        let mut rest = html[from..].trim_start();
        // Skip a closing quote of a quoted key (`"sitekey"` / `'sitekey'`).
        if rest.starts_with('"') || rest.starts_with('\'') {
            rest = rest[1..].trim_start();
        }
        let Some(after_sep) = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':')) else {
            continue;
        };
        let after_sep = after_sep.trim_start();
        let Some(q) = after_sep.chars().next() else {
            continue;
        };
        if q != '"' && q != '\'' {
            continue;
        }
        let body = &after_sep[q.len_utf8()..];
        if let Some(end) = body.find(q) {
            let v = body[..end].trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

// The `render=<sitekey>` query param on an api.js/enterprise.js src. Ignores the
// `explicit`/`onload` sentinels (which mean "render manually", not a key).
fn render_param(html: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = html[from..].find("render=") {
        let start = from + i + "render=".len();
        from = start;
        let token: String = html[start..]
            .chars()
            .take_while(|&c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            .collect();
        if !token.is_empty() && token != "explicit" && token != "onload" {
            return Some(token);
        }
    }
    None
}

// The first argument of `grecaptcha.execute('KEY', …)` (v3's programmatic call).
fn execute_first_arg(html: &str) -> Option<String> {
    let i = html.find("grecaptcha.execute(")?;
    let rest = html[i + "grecaptcha.execute(".len()..].trim_start();
    let q = rest.chars().next()?;
    if q != '"' && q != '\'' {
        return None;
    }
    let body = &rest[q.len_utf8()..];
    let end = body.find(q)?;
    let v = body[..end].trim().to_string();
    (!v.is_empty()).then_some(v)
}

// --- solver -----------------------------------------------------------------

/// Solves reCAPTCHA by running the page's own reCAPTCHA integration in the V8 render
/// tier (via an injected [`RecaptchaEngine`]) and harvesting the client
/// `g-recaptcha-response` token. See the module docs for the honest scope boundary.
pub struct RecaptchaSolver {
    /// The render-tier engine that runs the in-isolate flow. `None` until the render
    /// crate injects a `V8RecaptchaEngine` (so `turbo-surf-core` stays render-free) —
    /// a solve without one returns [`SolveError::NotConfigured`].
    engine: Option<Box<dyn RecaptchaEngine>>,
    client: http::Client,
}

impl RecaptchaSolver {
    pub fn new() -> Self {
        Self {
            engine: None,
            client: crate::net::build_client(),
        }
    }

    /// Inject the render tier's V8 reCAPTCHA engine — the in-isolate token flow.
    pub fn with_engine(mut self, engine: Box<dyn RecaptchaEngine>) -> Self {
        self.engine = Some(engine);
        self
    }
}

impl Default for RecaptchaSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for RecaptchaSolver {
    fn name(&self) -> &'static str {
        "recaptcha"
    }

    async fn solve(&self, ch: &Challenge, _ctx: &SolveContext) -> Result<SolvedToken, SolveError> {
        // Fetch the wall/page so we can classify the widget and hand its real markup to
        // the isolate. (Mirrors CloudflareSolver, which re-fetches the interstitial.)
        let page = self
            .client
            .get(&ch.page_url)
            .send()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?
            .text()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;

        // A v2 image-grid checkbox is not solvable in-isolate — surface it honestly so
        // the caller can route to an external solver (or the browser sidecar).
        if is_visual_v2_challenge(&page) {
            return Err(SolveError::VisualChallenge);
        }

        let engine = self.engine.as_ref().ok_or(SolveError::NotConfigured)?;
        let sitekey = ch
            .sitekey
            .clone()
            .or_else(|| extract_sitekey(&page))
            .ok_or_else(|| SolveError::Parse("recaptcha: no site key found on the page".into()))?;
        let action = ch
            .action
            .clone()
            .or_else(|| extract_action(&page))
            .unwrap_or_default();

        let token = engine
            .execute(&page, &ch.page_url, &sitekey, &action)
            .await
            .map_err(SolveError::Parse)?;
        if token.trim().is_empty() {
            // The VM ran but produced no token — the flow escalated to a challenge the
            // isolate can't clear (score gate / visual). Honest failure, not a silent "".
            return Err(SolveError::Parse(
                "recaptcha: no client token produced (score/visual gate)".into(),
            ));
        }

        Ok(SolvedToken {
            // The token is a FORM FIELD, not a cookie or header — surfaced under the
            // conventional `g-recaptcha-response` key so the caller replays it as the
            // widget response. reCAPTCHA tokens are single-use and short-lived (~2 min).
            cookies: Vec::new(),
            headers: vec![("g-recaptcha-response".to_string(), token)],
            ttl: Duration::from_secs(110),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::challenge::Vendor;

    #[test]
    fn extracts_sitekey_from_each_shape() {
        // data-sitekey attribute (v2/v3 widget div).
        assert_eq!(
            extract_sitekey(r#"<div class="g-recaptcha" data-sitekey="6LdKEY_widget"></div>"#),
            Some("6LdKEY_widget".to_string())
        );
        // render= query param on api.js (v3 programmatic).
        assert_eq!(
            extract_sitekey(
                r#"<script src="https://www.google.com/recaptcha/api.js?render=6LdKEY_v3"></script>"#
            ),
            Some("6LdKEY_v3".to_string())
        );
        // grecaptcha.render({ sitekey: '…' }) literal.
        assert_eq!(
            extract_sitekey(
                r#"grecaptcha.render('c', { 'sitekey': "6LdKEY_render", theme: 'light' });"#
            ),
            Some("6LdKEY_render".to_string())
        );
        // grecaptcha.execute('KEY', …) first argument.
        assert_eq!(
            extract_sitekey(r#"grecaptcha.execute('6LdKEY_exec', {action: 'login'});"#),
            Some("6LdKEY_exec".to_string())
        );
        // A page with none.
        assert_eq!(extract_sitekey("<html><body>ok</body></html>"), None);
        // `render=explicit`/`onload` are sentinels, not keys.
        assert_eq!(
            extract_sitekey(
                r#"<script src="/recaptcha/api.js?onload=cb&render=explicit"></script>"#
            ),
            None
        );
    }

    #[test]
    fn extracts_action_and_ignores_lookalikes() {
        assert_eq!(
            extract_action(r#"grecaptcha.execute(k, { action: 'submit_form' });"#),
            Some("submit_form".to_string())
        );
        // `transaction:` must NOT be read as `action`.
        assert_eq!(extract_action(r#"{ transaction: 'nope' }"#), None);
    }

    #[test]
    fn classifies_widget_kinds() {
        assert_eq!(
            classify(r#"<script src="/recaptcha/api.js?render=KEY123"></script>"#),
            RecaptchaKind::V3
        );
        assert_eq!(
            classify(r#"<div class="g-recaptcha" data-sitekey="K" data-size="invisible"></div>"#),
            RecaptchaKind::InvisibleV2
        );
        assert_eq!(
            classify(r#"<div class="g-recaptcha" data-sitekey="K"></div>"#),
            RecaptchaKind::CheckboxV2
        );
        // Only the checkbox widget is the unsolvable visual case.
        assert!(is_visual_v2_challenge(
            r#"<div class="g-recaptcha" data-sitekey="K"></div>"#
        ));
        assert!(!is_visual_v2_challenge(
            r#"<div class="g-recaptcha" data-sitekey="K" data-size="invisible"></div>"#
        ));
        assert!(!is_visual_v2_challenge(
            r#"<script src="/recaptcha/api.js?render=KEY123"></script>"#
        ));
    }

    // The v2 image-grid boundary, at the solver level: a checkbox widget is refused
    // with the documented error BEFORE any engine/network token attempt. A localhost
    // server stands in for the wall so the test stays offline.
    #[tokio::test]
    async fn solve_refuses_v2_image_checkbox() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf).await;
                let body = r#"<!DOCTYPE html><html><body>
                    <form><div class="g-recaptcha" data-sitekey="6Lc_checkbox_key"></div></form>
                    <script src="https://www.google.com/recaptcha/api.js"></script>
                    </body></html>"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        // No engine needed — the visual case is refused before the engine is consulted.
        let solver = RecaptchaSolver::new();
        let ch = Challenge::new(Vendor::Recaptcha, format!("http://127.0.0.1:{port}/"));
        let err = solver.solve(&ch, &SolveContext::default()).await;
        assert!(
            matches!(err, Err(SolveError::VisualChallenge)),
            "v2 image checkbox must return the documented VisualChallenge error, got {err:?}"
        );
    }

    // Without an injected engine, an otherwise-solvable v3 page reports NotConfigured
    // (the render tier must supply the isolate) — never a silent success.
    #[tokio::test]
    async fn solve_without_engine_is_not_configured() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf).await;
                let body = r#"<!DOCTYPE html><html><head>
                    <script src="https://www.google.com/recaptcha/api.js?render=6Lc_v3_key"></script>
                    </head><body>ok</body></html>"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        let solver = RecaptchaSolver::new();
        let ch = Challenge::new(Vendor::Recaptcha, format!("http://127.0.0.1:{port}/"));
        assert!(matches!(
            solver.solve(&ch, &SolveContext::default()).await,
            Err(SolveError::NotConfigured)
        ));
    }
}
