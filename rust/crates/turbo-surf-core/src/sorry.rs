//! Google **`/sorry` → SERP clearance loop**. When `/search` is served to a
//! flagged / headless client, Google 302s to
//! `/sorry/index?continue=<SERP-url>&q=<token>…` — a reCAPTCHA form. This module
//! wires the full round-trip:
//!
//! 1. Parse the `/sorry` form (POST endpoint, the `continue` SERP URL, hidden fields).
//! 2. Take a `g-recaptcha-response` token — **from any source** (the in-isolate
//!    [`crate::recaptcha::RecaptchaSolver`] for v3/invisible, or an external solver
//!    for a v2 image grid). The loop itself is token-source-agnostic.
//! 3. POST the token + hidden fields (form-encoded) to `/sorry/index`.
//! 4. Capture the **`GOOGLE_ABUSE_EXEMPTION`** `Set-Cookie` into the jar.
//! 5. Re-fetch the `continue` (SERP) URL with that cookie → the cleared SERP HTML.
//!
//! The exemption cookie stays in the caller's [`CookieJar`], so subsequent requests
//! to Google stay cleared for its lifetime.
//!
//! ## Honest scope
//!
//! The loop is real and clears the wall **once a valid token exists**. LIVE Google
//! `/sorry` is usually a **v2 image-grid** challenge, whose token needs an external
//! solver ([`SolveError::VisualChallenge`] is surfaced so the caller can defer);
//! [`crate::recaptcha::RecaptchaSolver`] only mints tokens for v3/invisible/score
//! flows, and those are accepted only with a clean egress IP. The reliable no-`/sorry`
//! path remains the headed browser sidecar.

use crate::challenge::{Challenge, ChallengeSolver, SolveContext, SolveError, Vendor};
use crate::cookies::CookieJar;
use crate::net::{self, FetchOptions};

/// The cookie Google sets to exempt a client from the unusual-traffic wall.
pub const EXEMPTION_COOKIE: &str = "GOOGLE_ABUSE_EXEMPTION";

/// A parsed `/sorry` reCAPTCHA form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SorryForm {
    /// The absolute POST endpoint (the form `action`, resolved against the page URL;
    /// typically `https://www.google.com/sorry/index`).
    pub action: String,
    /// The `continue` field — the original SERP URL to return to after clearance.
    pub continue_url: Option<String>,
    /// Every hidden field to replay (`continue`, `q`, `hl`, …), in document order.
    pub fields: Vec<(String, String)>,
}

/// The result of a successful clearance: the SERP HTML plus the captured exemption.
#[derive(Debug, Clone)]
pub struct SorryCleared {
    /// The re-fetched SERP HTML (what the caller wanted all along).
    pub serp_html: String,
    /// The SERP URL actually landed on (after redirects).
    pub final_url: String,
    /// The captured `GOOGLE_ABUSE_EXEMPTION` (name, value), if the POST minted one.
    pub exemption: Option<(String, String)>,
}

/// Is this response the google `/sorry` unusual-traffic wall (as opposed to an
/// ordinary page that merely embeds a reCAPTCHA widget)? Keys on the `/sorry` URL
/// marker or the unusual-traffic body phrase — the same signals
/// [`crate::challenge::detect`] uses to flag [`Vendor::Recaptcha`].
pub fn is_sorry_wall(page_url: &str, body: &str) -> bool {
    page_url.contains("/sorry/")
        || body.contains("our systems have detected unusual traffic")
        || body.contains("unusual traffic from your computer network")
}

// --- form parsing -----------------------------------------------------------

/// Parse the `/sorry` form out of its HTML: the POST `action`, the `continue` SERP
/// URL, and the hidden fields to replay. Manual string parsing (this crate stays
/// tree-parser-free — see [`crate::recaptcha`]). `None` if there is no `<form>`.
pub fn parse_sorry_form(page_url: &str, html: &str) -> Option<SorryForm> {
    let lower = html.to_ascii_lowercase();
    let form_start = lower.find("<form")?;
    let form_end = lower[form_start..]
        .find("</form>")
        .map(|e| form_start + e)
        .unwrap_or(html.len());
    let region = &html[form_start..form_end];

    // The `<form …>` open tag carries the POST `action`; resolve it against the page.
    let open_end = region.find('>').unwrap_or(region.len());
    let open_tag = &region[..open_end];
    let action_raw = attr_value(open_tag, "action").unwrap_or_default();
    let action = if action_raw.is_empty() {
        page_url.to_string()
    } else {
        let unescaped = html_unescape(&action_raw);
        crate::url::resolve(page_url, &unescaped).unwrap_or(unescaped)
    };

    // Every `<input name=… value=…>` in the form. The reCAPTCHA response placeholder
    // is dropped — the loop supplies its own token.
    let mut fields = Vec::new();
    let mut continue_url = None;
    let region_lower = region.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = region_lower[from..].find("<input") {
        let start = from + i;
        let end = region[start..]
            .find('>')
            .map(|e| start + e + 1)
            .unwrap_or(region.len());
        let tag = &region[start..end];
        from = end;
        let Some(name) = attr_value(tag, "name") else {
            continue;
        };
        if name.eq_ignore_ascii_case("g-recaptcha-response") {
            continue;
        }
        let value = html_unescape(&attr_value(tag, "value").unwrap_or_default());
        if name.eq_ignore_ascii_case("continue") {
            continue_url = Some(value.clone());
        }
        fields.push((name, value));
    }
    Some(SorryForm {
        action,
        continue_url,
        fields,
    })
}

// Value of a `name="…"` / `name='…'` / `name=bare` attribute in a single HTML tag.
// Requires `name` to sit at an attribute boundary (preceded by whitespace or `<`)
// so `data-name` doesn't match `name`. Shared with [`crate::consent`] (the consent
// handshake parses the same shape of hidden-field form).
pub(crate) fn attr_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let idx = from + i;
        from = idx + name.len();
        let boundary = tag[..idx]
            .chars()
            .next_back()
            .map(|c| c.is_whitespace() || c == '<')
            .unwrap_or(true);
        if !boundary {
            continue;
        }
        let rest = tag[idx + name.len()..].trim_start();
        let Some(after) = rest.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let Some(q) = after.chars().next() else {
            continue;
        };
        if q == '"' || q == '\'' {
            let body = &after[q.len_utf8()..];
            let end = body.find(q)?;
            return Some(body[..end].to_string());
        }
        // Unquoted value: up to the next whitespace or tag close.
        let v: String = after
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/')
            .collect();
        if !v.is_empty() {
            return Some(v);
        }
    }
    None
}

// Minimal HTML entity unescape for attribute values — chiefly `&amp;` in the
// `continue` URL's query string (the only entity Google emits there in practice).
// Shared with [`crate::consent`].
pub(crate) fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&#x26;", "&")
}

// --- the clearance loop -----------------------------------------------------

// Form-encode the hidden fields plus the supplied token as `g-recaptcha-response`.
fn form_body(fields: &[(String, String)], token: &str) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in fields {
        ser.append_pair(k, v);
    }
    ser.append_pair("g-recaptcha-response", token);
    ser.finish()
}

/// **The token-source-agnostic loop.** POST `token` + the form's hidden fields to
/// the `/sorry/index` endpoint, capture the `GOOGLE_ABUSE_EXEMPTION` `Set-Cookie`
/// into `jar`, and re-fetch the `continue` SERP with it — returning the cleared HTML.
///
/// Works with **any** valid `g-recaptcha-response`, whatever minted it. An empty
/// token is rejected before the POST (so a missing token never sets the cookie), and
/// a POST that yields no exemption cookie is a clear error rather than a silent miss.
pub async fn submit_and_clear(
    jar: &mut CookieJar,
    form: &SorryForm,
    token: &str,
    user_agent: &str,
    now: f64,
) -> Result<SorryCleared, SolveError> {
    if token.trim().is_empty() {
        return Err(SolveError::Parse(
            "sorry: refusing to POST an empty g-recaptcha-response token".into(),
        ));
    }
    let continue_url = form
        .continue_url
        .clone()
        .ok_or_else(|| SolveError::Parse("sorry: form has no `continue` (SERP) URL".into()))?;

    // 1. POST the token + hidden fields. Don't auto-follow the 302: we want to ingest
    //    the exemption Set-Cookie ourselves before returning to the SERP. `fetch_html`
    //    with `max_redirects: Some(0)` still ingests Set-Cookie on that hop.
    let mut headers = std::collections::BTreeMap::new();
    headers.insert(
        "content-type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );
    if !user_agent.is_empty() {
        headers.insert("user-agent".to_string(), user_agent.to_string());
    }
    let post = FetchOptions {
        method: Some("POST".to_string()),
        body: Some(form_body(&form.fields, token)),
        headers,
        allow_non_html: true,
        max_redirects: Some(0),
        jar: Some(jar),
        now,
        ..Default::default()
    };
    net::fetch_html(&form.action, post)
        .await
        .map_err(|e| SolveError::Http(e.to_string()))?;

    // 2. Capture the exemption cookie from the jar (as it applies to the SERP URL).
    let exemption = jar
        .cookies_for(&continue_url, now)
        .into_iter()
        .find(|c| c.name == EXEMPTION_COOKIE)
        .map(|c| (c.name, c.value));
    if exemption.is_none() {
        return Err(SolveError::Parse(format!(
            "sorry: no {EXEMPTION_COOKIE} cookie after the /sorry POST (token rejected?)"
        )));
    }

    // 3. Re-fetch the continue SERP; the jar now replays the exemption cookie.
    let serp = net::fetch_html(
        &continue_url,
        FetchOptions {
            jar: Some(jar),
            now,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| SolveError::Http(e.to_string()))?;

    Ok(SorryCleared {
        serp_html: serp.html,
        final_url: serp.final_url,
        exemption,
    })
}

/// Full orchestration: detect a `/sorry` wall on `(page_url, html)`; if present,
/// obtain a token via `solver` and run [`submit_and_clear`]. `Ok(None)` when the
/// response is not a `/sorry` wall (an ordinary page — nothing to clear).
///
/// Token-source-agnostic via the [`ChallengeSolver`] seam: the in-isolate
/// [`crate::recaptcha::RecaptchaSolver`] mints a `g-recaptcha-response` for
/// v3/invisible flows; a v2 image grid surfaces [`SolveError::VisualChallenge`] from
/// the solver, which propagates so the caller can defer to an external solver. If the
/// solver instead returns cleared cookies (a browser/external solver that renders the
/// whole flow and hands back `GOOGLE_ABUSE_EXEMPTION`), those are ingested directly.
pub async fn clear_if_sorry(
    solver: &dyn ChallengeSolver,
    jar: &mut CookieJar,
    page_url: &str,
    html: &str,
    ctx: &SolveContext,
    now: f64,
) -> Result<Option<SorryCleared>, SolveError> {
    if !is_sorry_wall(page_url, html) {
        return Ok(None);
    }
    let form = parse_sorry_form(page_url, html)
        .ok_or_else(|| SolveError::Parse("sorry: no <form> on the /sorry page".into()))?;

    let ch = Challenge {
        vendor: Vendor::Recaptcha,
        page_url: page_url.to_string(),
        sitekey: crate::recaptcha::extract_sitekey(html),
        action: crate::recaptcha::extract_action(html),
    };
    // VisualChallenge (v2 image grid) / NotConfigured propagate to the caller.
    let solved = solver.solve(&ch, ctx).await?;

    // Prefer a minted token (the in-isolate / captcha-service path).
    if let Some(token) = solved
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("g-recaptcha-response"))
        .map(|(_, v)| v.clone())
        .filter(|t| !t.trim().is_empty())
    {
        return submit_and_clear(jar, &form, &token, &ctx.user_agent, now)
            .await
            .map(Some);
    }

    // Otherwise the solver cleared the wall itself and handed back cookies — ingest
    // them (the exemption among them) and re-fetch the continue SERP.
    if solved
        .cookies
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case(EXEMPTION_COOKIE))
    {
        let continue_url = form
            .continue_url
            .clone()
            .ok_or_else(|| SolveError::Parse("sorry: form has no `continue` (SERP) URL".into()))?;
        let host = crate::url::host_of(&continue_url).unwrap_or_default();
        for (k, v) in &solved.cookies {
            jar.add(k, v, &host, "/", None);
        }
        let exemption = solved
            .cookies
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(EXEMPTION_COOKIE))
            .map(|(k, v)| (k.clone(), v.clone()));
        let serp = net::fetch_html(
            &continue_url,
            FetchOptions {
                jar: Some(jar),
                now,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| SolveError::Http(e.to_string()))?;
        return Ok(Some(SorryCleared {
            serp_html: serp.html,
            final_url: serp.final_url,
            exemption,
        }));
    }

    Err(SolveError::Parse(
        "sorry: solver produced neither a g-recaptcha-response token nor an exemption cookie"
            .into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::challenge::SolvedToken;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const SORRY_FORM: &str = r#"<!DOCTYPE html><html><body>
        <p>Our systems have detected unusual traffic from your computer network.</p>
        <form action="/sorry/index" method="post">
          <input type="hidden" name="continue" value="CONTINUE_URL">
          <input type="hidden" name="q" value="EgS_challenge_token">
          <input type="hidden" name="hl" value="en">
          <div class="g-recaptcha" data-sitekey="6Lc_sorry_key"></div>
        </form>
        <script src="https://www.google.com/recaptcha/api.js"></script>
        </body></html>"#;

    #[test]
    fn parses_sorry_form_fields_and_action() {
        let form =
            parse_sorry_form("https://www.google.com/sorry/index?continue=x", SORRY_FORM).unwrap();
        assert_eq!(form.action, "https://www.google.com/sorry/index");
        assert_eq!(form.continue_url.as_deref(), Some("CONTINUE_URL"));
        assert!(form
            .fields
            .contains(&("q".to_string(), "EgS_challenge_token".to_string())));
        assert!(form.fields.contains(&("hl".to_string(), "en".to_string())));
        // The reCAPTCHA response placeholder must NOT be replayed as a hidden field.
        assert!(!form
            .fields
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("g-recaptcha-response")));
    }

    #[test]
    fn unescapes_amp_in_continue_url() {
        let html = r#"<form action="/sorry/index"><input name="continue"
            value="https://www.google.com/search?q=cats&amp;hl=en&amp;num=10"></form>"#;
        let form = parse_sorry_form("https://www.google.com/sorry/index", html).unwrap();
        assert_eq!(
            form.continue_url.as_deref(),
            Some("https://www.google.com/search?q=cats&hl=en&num=10")
        );
    }

    #[test]
    fn detects_sorry_wall() {
        assert!(is_sorry_wall(
            "https://www.google.com/sorry/index?continue=x",
            ""
        ));
        assert!(is_sorry_wall(
            "https://x.test/",
            "our systems have detected unusual traffic here"
        ));
        assert!(!is_sorry_wall(
            "https://x.test/",
            "<html>ordinary page</html>"
        ));
    }

    // A solver stub that just returns a canned g-recaptcha-response token — stands in
    // for the in-isolate engine (or any captcha service) so the loop test stays offline.
    struct TokenSolver(&'static str);
    #[async_trait::async_trait]
    impl ChallengeSolver for TokenSolver {
        fn name(&self) -> &'static str {
            "token-stub"
        }
        async fn solve(
            &self,
            _ch: &Challenge,
            _ctx: &SolveContext,
        ) -> Result<SolvedToken, SolveError> {
            Ok(SolvedToken {
                cookies: Vec::new(),
                headers: vec![("g-recaptcha-response".to_string(), self.0.to_string())],
                ttl: Duration::from_secs(110),
            })
        }
    }

    // A solver stub that surfaces the v2 image-grid boundary.
    struct VisualSolver;
    #[async_trait::async_trait]
    impl ChallengeSolver for VisualSolver {
        fn name(&self) -> &'static str {
            "visual-stub"
        }
        async fn solve(
            &self,
            _ch: &Challenge,
            _ctx: &SolveContext,
        ) -> Result<SolvedToken, SolveError> {
            Err(SolveError::VisualChallenge)
        }
    }

    // A localhost server that plays google's `/search` + `/sorry/index` + cleared
    // `/search`. `/search` with NO exemption cookie 302s to `/sorry/index`; the
    // `/sorry` page carries the reCAPTCHA form; POST `/sorry/index` with a non-empty
    // `g-recaptcha-response` sets `GOOGLE_ABUSE_EXEMPTION` + 302s to `continue`;
    // `/search` WITH the cookie returns the real SERP HTML.
    async fn spawn_google_fixture() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let base_for_task = base.clone();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let base = base_for_task.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = req.lines().next().unwrap_or("");
                    let has_exemption = req.contains("GOOGLE_ABUSE_EXEMPTION=");
                    let is_post = line.starts_with("POST");
                    // A real (non-empty) g-recaptcha-response in the POST body.
                    let has_token = req.contains("g-recaptcha-response=")
                        && !req.contains("g-recaptcha-response=&")
                        && !req.trim_end().ends_with("g-recaptcha-response=");

                    let resp = if line.starts_with("GET /search") && has_exemption {
                        // Cleared: the real SERP.
                        let body = r#"<!DOCTYPE html><html><body><div id="search">
                            <a href="https://example.com/1"><h3>Real Result One</h3></a>
                            </div></body></html>"#;
                        http_ok(body)
                    } else if line.starts_with("GET /search") {
                        // Flagged: bounce to /sorry.
                        let cont = format!("{base}/search?q=cats");
                        let enc: String =
                            url::form_urlencoded::byte_serialize(cont.as_bytes()).collect();
                        format!(
                            "HTTP/1.1 302 Found\r\nLocation: /sorry/index?continue={enc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                    } else if line.starts_with("GET /sorry") {
                        // The /sorry reCAPTCHA form, with a live continue back to /search.
                        let cont = format!("{base}/search?q=cats");
                        let body = format!(
                            r#"<!DOCTYPE html><html><body>
                            <p>Our systems have detected unusual traffic from your computer network.</p>
                            <form action="/sorry/index" method="post">
                              <input type="hidden" name="continue" value="{cont}">
                              <input type="hidden" name="q" value="challenge_tok">
                              <input type="hidden" name="hl" value="en">
                              <div class="g-recaptcha" data-sitekey="6Lc_sorry"></div>
                            </form></body></html>"#
                        );
                        http_ok(&body)
                    } else if is_post && line.contains("/sorry/index") && has_token {
                        // Valid token → mint the exemption cookie + bounce to continue.
                        let cont = format!("{base}/search?q=cats");
                        format!(
                            "HTTP/1.1 302 Found\r\nSet-Cookie: GOOGLE_ABUSE_EXEMPTION=ABUSE_OK; Path=/\r\nLocation: {cont}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                    } else if is_post && line.contains("/sorry/index") {
                        // POST with no/empty token → NO cookie, back to the form.
                        http_ok("<html><body>no token</body></html>")
                    } else {
                        http_ok("<html><body>404</body></html>")
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        (base, handle)
    }

    fn http_ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    // The full loop: fetch /search → 302 /sorry → parse form → (stub) token → POST →
    // capture GOOGLE_ABUSE_EXEMPTION → re-fetch /search → the real SERP HTML.
    #[tokio::test]
    async fn full_sorry_to_serp_loop_yields_serp() {
        let (base, _srv) = spawn_google_fixture().await;
        let mut jar = CookieJar::new();

        // Initial /search hits the wall (lands on /sorry).
        let first = net::fetch_html(
            &format!("{base}/search?q=cats"),
            FetchOptions {
                jar: Some(&mut jar),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(first.final_url.contains("/sorry"), "should land on /sorry");
        assert!(is_sorry_wall(&first.final_url, &first.html));

        let cleared = clear_if_sorry(
            &TokenSolver("VALID_TOKEN_123"),
            &mut jar,
            &first.final_url,
            &first.html,
            &SolveContext::default(),
            0.0,
        )
        .await
        .unwrap()
        .expect("a /sorry wall should be cleared");

        assert!(
            cleared.serp_html.contains("Real Result One"),
            "cleared SERP HTML expected, got: {}",
            cleared.serp_html
        );
        assert_eq!(
            cleared.exemption,
            Some(("GOOGLE_ABUSE_EXEMPTION".to_string(), "ABUSE_OK".to_string()))
        );
        // The exemption cookie persists in the caller's jar for subsequent requests.
        assert!(jar
            .cookies_for(&format!("{base}/search"), 0.0)
            .iter()
            .any(|c| c.name == EXEMPTION_COOKIE));
    }

    // The v2 image-grid boundary: the solver surfaces VisualChallenge, which the loop
    // propagates unchanged so the caller can defer to an external solver.
    #[tokio::test]
    async fn v2_image_challenge_defers() {
        let (base, _srv) = spawn_google_fixture().await;
        let mut jar = CookieJar::new();
        let sorry_url = format!("{base}/sorry/index?continue=x");
        let sorry_html = format!(
            r#"<form action="/sorry/index" method="post">
               <input name="continue" value="{base}/search?q=cats">
               <div class="g-recaptcha" data-sitekey="K"></div></form>
               <p>unusual traffic from your computer network</p>"#
        );
        let err = clear_if_sorry(
            &VisualSolver,
            &mut jar,
            &sorry_url,
            &sorry_html,
            &SolveContext::default(),
            0.0,
        )
        .await;
        assert!(
            matches!(err, Err(SolveError::VisualChallenge)),
            "v2 image grid must surface VisualChallenge for deferral, got {err:?}"
        );
    }

    // A missing/empty token must NOT set the exemption cookie (and must error).
    #[tokio::test]
    async fn missing_token_sets_no_cookie() {
        let (base, _srv) = spawn_google_fixture().await;
        let mut jar = CookieJar::new();
        let form = SorryForm {
            action: format!("{base}/sorry/index"),
            continue_url: Some(format!("{base}/search?q=cats")),
            fields: vec![("q".to_string(), "tok".to_string())],
        };
        let err = submit_and_clear(&mut jar, &form, "   ", "", 0.0).await;
        assert!(
            matches!(err, Err(SolveError::Parse(_))),
            "empty token must be refused, got {err:?}"
        );
        assert!(
            jar.cookies_for(&format!("{base}/search"), 0.0)
                .iter()
                .all(|c| c.name != EXEMPTION_COOKIE),
            "no exemption cookie may be set without a token"
        );
    }
}
