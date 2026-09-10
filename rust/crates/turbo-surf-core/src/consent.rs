//! Consent-wall handling for Google (and EU-region CMPs). A fresh visitor in a
//! consent region is served the **"Before you continue to Google"** interstitial
//! before the homepage / SERP. There are two levels of handling here:
//!
//! 1. **Synthetic dismissal** ([`cookies_for_host`]) — a hard-coded minimal `SOCS`
//!    cookie seeded onto the request so the interstitial is skipped and the real,
//!    server-rendered page is served. Cheap, cookie-only, no round-trip. This
//!    *un-hides the page* but does NOT establish a trusted session.
//!
//! 2. **The real consent handshake** ([`handshake_if_consent`]) — replay the
//!    interstitial's own **"Accept all"** form to `consent.google.com/save`, exactly
//!    as a browser does when the user clicks the button, and ingest the `Set-Cookie`s
//!    that response mints into the caller's [`CookieJar`]. This is what earns
//!    **`NID`** (Google's preferences / trusted-session cookie) — see the module note
//!    below.
//!
//! ## Why the handshake matters: what mints `NID`
//!
//! Reverse-engineering (CDP trace of a real Chrome consent flow) established the
//! chain precisely:
//!
//! - A raw homepage GET Set-Cookies only `AEC` + `__Secure-ENID` — never `NID`.
//! - The interstitial's **"Accept all"** button submits a form to
//!   `https://consent.google.com/save` carrying `continue`, `gl`, `hl`, `pc`, `bl`,
//!   the accept selectors `set_eom=false&set_aps=true&set_sc=true`, and a signed
//!   `escs` token embedded in the page.
//! - **That `/save` response is the one that `Set-Cookie: NID` (+ `SOCS`,
//!   `SEARCH_SAMESITE`, `__Secure-STRP`)** and 302s back to `continue`.
//!
//! So `NID` is minted by the *consent-save handshake* — not by `document.cookie`, not
//! by a `/gen_204` ping, not by the homepage response itself. The synthetic `SOCS`
//! shortcut skips this handshake, which is why it dismisses the wall but leaves
//! `/search` on the JS-gated `enablejs` shell (no trusted session). The handshake
//! runs the *real* accept so the jar gains the real `NID`.
//!
//! ## Honest scope
//!
//! The handshake authentically replays the accept form and ingests whatever cookies
//! Google mints in response — on a clean egress IP this is the same request the
//! browser makes, so it earns the same `NID`. Whether `/search` then returns a SERP
//! (vs. bouncing to the `/sorry` unusual-traffic wall) depends on the egress IP
//! reputation, which is orthogonal to the cookie: a flagged IP gets `/sorry` even
//! *with* a valid `NID` (that path is handled by [`crate::sorry`]).

use crate::cookies::CookieJar;
use crate::net::{self, FetchOptions};
use crate::sorry::{attr_value, html_unescape};

/// Google's preferences / trusted-session cookie — the one the consent handshake
/// mints and that a JS-free `/search` needs to return a real SERP.
pub const NID_COOKIE: &str = "NID";

/// Consent cookies (`name`, `value`) to send for `host`, or empty if the host has
/// no known consent wall. Matched on a host substring so country TLDs
/// (`google.co.uk`, `google.de`) and subdomains are covered.
pub fn cookies_for_host(host: &str) -> &'static [(&'static str, &'static str)] {
    let h = host.to_ascii_lowercase();
    // `SOCS` dismisses google's "Before you continue to Google" interstitial so the
    // real homepage / SERP (search box, results, footer) is served un-hidden.
    if h.contains("google.") || h.contains("youtube.") {
        return &[("SOCS", "CAESHAgBEhIaAB")];
    }
    &[]
}

// --- the real consent handshake --------------------------------------------

/// A parsed **"Accept all"** consent form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentForm {
    /// The absolute submit endpoint (the form `action`, resolved against the page
    /// URL; typically `https://consent.google.com/save`).
    pub action: String,
    /// The HTTP method (`GET` or `POST`), upper-cased. Defaults to `GET` per the HTML
    /// spec when the form omits `method`; Google's forms specify `POST`.
    pub method: String,
    /// Every field to replay (`continue`, `gl`, `hl`, `bl`, `escs`, the `set_*`
    /// accept selectors, …), in document order.
    pub fields: Vec<(String, String)>,
}

/// The outcome of a successful handshake.
#[derive(Debug, Clone)]
pub struct ConsentEarned {
    /// The `continue` URL the accept form pointed back to (the page the caller
    /// originally wanted).
    pub continue_url: Option<String>,
    /// Whether the jar now carries an `NID` cookie applicable to Google (the trusted
    /// session was minted).
    pub earned_nid: bool,
    /// The names of all cookies the handshake response added to the jar (for
    /// diagnostics / recon).
    pub minted: Vec<String>,
}

/// Is this response Google's "Before you continue" consent interstitial (as opposed
/// to an ordinary page)? Keys on the consent-save form action, the consent host, or
/// the interstitial's body phrase — the signals a browser's user would see the
/// Accept/Reject buttons on.
pub fn is_consent_interstitial(page_url: &str, body: &str) -> bool {
    if page_url.contains("consent.google.com") || page_url.contains("consent.youtube.com") {
        return true;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("consent.google.com/save")
        || lower.contains("consent.youtube.com/save")
        || (lower.contains("before you continue")
            && (lower.contains("google") || lower.contains("youtube")))
}

/// Parse the **"Accept all"** form out of the consent interstitial HTML. The page
/// carries several forms (Reject all / Accept all / More options); the accept form is
/// the one whose selector fields say *accept* (`set_aps=true` **and** `set_sc=true`).
/// Manual string parsing (this crate stays tree-parser-free — same as
/// [`crate::sorry`]). `None` when no accept form is present.
pub fn parse_accept_form(page_url: &str, html: &str) -> Option<ConsentForm> {
    // Older/simple interstitials use a real <input> form.
    if let Some(f) = iter_forms(page_url, html).into_iter().find(is_accept_form) {
        return Some(f);
    }
    // Live google consent page has NO accept <form> — the "Accept all" target is a JS
    // string, hex-escaped: `var rAU='https://consent.google.com/save?...set_aps\x3dtrue
    // \x26set_sc\x3dtrue...\x26escs\x3d<token>'`. All params (incl. the signed escs and
    // `continue`) are already in the query, so it's a plain GET. Pick the accept variant
    // (set_aps=true & set_sc=true), not the reject one (set_eom=true).
    extract_js_save_url(html).map(|action| ConsentForm {
        action,
        method: "GET".to_string(),
        fields: Vec::new(),
    })
}

// Extract google's consent `/save` "accept all" URL from the interstitial's inline JS,
// decoding `\xHH` escapes. Returns the first URL whose (decoded) query selects accept.
fn extract_js_save_url(html: &str) -> Option<String> {
    let needle = "https://consent.google.com/save?";
    let mut from = 0;
    while let Some(i) = html[from..].find(needle) {
        let start = from + i;
        let rest = &html[start..];
        // The JS string literal ends at the next quote.
        let end = rest.find(['\'', '"']).unwrap_or(rest.len());
        from = start + end;
        let url = js_unescape(&rest[..end]);
        let q = url.to_ascii_lowercase();
        if q.contains("set_aps=true") && q.contains("set_sc=true") {
            return Some(url);
        }
    }
    None
}

// Decode JS `\xHH` hex escapes (Google encodes `=`→`\x3d`, `&`→`\x26` in the save URL).
// Non-escape bytes pass through; the payload is ASCII.
fn js_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 4 <= b.len() && b[i + 1] == b'x' {
            if let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                out.push(v as char);
                i += 4;
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

// True when a form's fields select "accept all": both `set_aps` and `set_sc` present
// and truthy. Reject-all carries the same field names set to `false`, so keying on the
// values (not mere presence) is what distinguishes accept from reject.
fn is_accept_form(form: &ConsentForm) -> bool {
    let truthy = |name: &str| {
        form.fields
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case(name) && v.eq_ignore_ascii_case("true"))
    };
    truthy("set_aps") && truthy("set_sc")
}

// Every `<form>…</form>` on the page, parsed into a [`ConsentForm`] (action + method +
// its `<input>` name/value fields). Reuses [`crate::sorry`]'s attribute/entity helpers.
fn iter_forms(page_url: &str, html: &str) -> Vec<ConsentForm> {
    let lower = html.to_ascii_lowercase();
    let mut forms = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<form") {
        let start = from + i;
        let end = lower[start..]
            .find("</form>")
            .map(|e| start + e)
            .unwrap_or(html.len());
        let region = &html[start..end];
        from = end + "</form>".len().min(html.len().saturating_sub(end));

        let open_end = region.find('>').unwrap_or(region.len());
        let open_tag = &region[..open_end];
        let action_raw = attr_value(open_tag, "action").unwrap_or_default();
        let action = if action_raw.is_empty() {
            page_url.to_string()
        } else {
            let unescaped = html_unescape(&action_raw);
            crate::url::resolve(page_url, &unescaped).unwrap_or(unescaped)
        };
        let method = attr_value(open_tag, "method")
            .unwrap_or_else(|| "GET".to_string())
            .to_ascii_uppercase();

        let mut fields = Vec::new();
        let region_lower = region.to_ascii_lowercase();
        let mut ifrom = 0;
        while let Some(j) = region_lower[ifrom..].find("<input") {
            let istart = ifrom + j;
            let iend = region[istart..]
                .find('>')
                .map(|e| istart + e + 1)
                .unwrap_or(region.len());
            let tag = &region[istart..iend];
            ifrom = iend;
            let Some(name) = attr_value(tag, "name") else {
                continue;
            };
            let value = html_unescape(&attr_value(tag, "value").unwrap_or_default());
            fields.push((name, value));
        }
        forms.push(ConsentForm {
            action,
            method,
            fields,
        });
    }
    forms
}

// The `continue` field of a form (the URL to return to after the handshake).
fn continue_of(form: &ConsentForm) -> Option<String> {
    form.fields
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("continue"))
        .map(|(_, v)| v.clone())
}

/// **Submit the accept form** exactly as the browser's "Accept all" click does, and
/// ingest the `Set-Cookie`s the response mints into `jar`. Honors the form's method:
/// a `POST` sends the fields form-encoded in the body; a `GET` appends them to the
/// action's query. Redirects are followed (the `/save` 302 → `continue`), and
/// `Set-Cookie` is ingested on every hop — so `NID` lands in the jar wherever Google
/// sets it.
pub async fn earn_consent_session(
    jar: &mut CookieJar,
    form: &ConsentForm,
    user_agent: &str,
    now: f64,
) -> Result<ConsentEarned, String> {
    let before: std::collections::HashSet<String> = jar
        .cookies_for(&form.action, now)
        .into_iter()
        .map(|c| c.name)
        .collect();

    let mut headers = std::collections::BTreeMap::new();
    if !user_agent.is_empty() {
        headers.insert("user-agent".to_string(), user_agent.to_string());
    }

    // Follow redirects MANUALLY: Google mints `NID` via `Set-Cookie` on the `/save`
    // **302** (which then bounces to `continue`). `fetch_html`'s auto-follow only
    // ingests `Set-Cookie` on the *final* response, so it would drop the 302's cookies
    // — `max_redirects` routes through the per-hop-ingest path (same reason
    // [`crate::sorry`] sets it).
    let (target, opts) = if form.method == "POST" {
        headers.insert(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        (
            form.action.clone(),
            FetchOptions {
                method: Some("POST".to_string()),
                body: Some(form_encode(&form.fields)),
                headers,
                allow_non_html: true,
                max_redirects: Some(5),
                jar: Some(jar),
                now,
                ..Default::default()
            },
        )
    } else {
        // GET: fold the fields into the action's query string.
        let target = append_query(&form.action, &form.fields);
        (
            target,
            FetchOptions {
                headers,
                allow_non_html: true,
                max_redirects: Some(5),
                jar: Some(jar),
                now,
                ..Default::default()
            },
        )
    };

    net::fetch_html(&target, opts)
        .await
        .map_err(|e| format!("consent handshake: {e}"))?;

    // What did the handshake add to the jar? (Applicable to the save endpoint, which
    // Google scopes to `.google.com`, so it also covers www/search.)
    let after = jar.cookies_for(&form.action, now);
    let minted: Vec<String> = after
        .iter()
        .filter(|c| !before.contains(&c.name))
        .map(|c| c.name.clone())
        .collect();
    let earned_nid = after.iter().any(|c| c.name == NID_COOKIE);

    Ok(ConsentEarned {
        continue_url: continue_of(form),
        earned_nid,
        minted,
    })
}

/// Full orchestration, mirroring [`crate::sorry::clear_if_sorry`]: if
/// `(page_url, html)` is the consent interstitial, parse the accept form and run
/// [`earn_consent_session`]. `Ok(None)` when the response is not a consent wall
/// (nothing to do). A consent wall with no parseable accept form is a clear error
/// rather than a silent miss.
pub async fn handshake_if_consent(
    jar: &mut CookieJar,
    page_url: &str,
    html: &str,
    user_agent: &str,
    now: f64,
) -> Result<Option<ConsentEarned>, String> {
    if !is_consent_interstitial(page_url, html) {
        return Ok(None);
    }
    let form = parse_accept_form(page_url, html).ok_or_else(|| {
        "consent: no parseable \"Accept all\" form on the interstitial".to_string()
    })?;
    earn_consent_session(jar, &form, user_agent, now)
        .await
        .map(Some)
}

// Form-encode fields for a POST body.
fn form_encode(fields: &[(String, String)]) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in fields {
        ser.append_pair(k, v);
    }
    ser.finish()
}

// Append fields to a URL's query string for a GET submit (preserving any query the
// action already carries).
fn append_query(action: &str, fields: &[(String, String)]) -> String {
    if fields.is_empty() {
        return action.to_string();
    }
    let enc = form_encode(fields);
    let sep = if action.contains('?') { '&' } else { '?' };
    format!("{action}{sep}{enc}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn parses_the_live_js_var_accept_url() {
        // Live google interstitial: no accept <form>; the target is a hex-escaped JS
        // string. Accept = set_aps/set_sc true; reject = set_eom true. Must pick accept.
        let html = r#"<!doctype html><html><body>
          <form action="/search" method="GET"><input name="q"></form>
          <script>var rAU='https://consent.google.com/save?continue\x3dhttps://www.google.com/\x26gl\x3dMT\x26set_eom\x3dtrue\x26escs\x3dREJECT';
          var rAU2='https://consent.google.com/save?continue\x3dhttps://www.google.com/\x26gl\x3dMT\x26set_eom\x3dfalse\x26set_aps\x3dtrue\x26set_sc\x3dtrue\x26escs\x3dACCEPTTOKEN';</script>
        </body></html>"#;
        let form = parse_accept_form("https://www.google.com/", html).expect("accept url");
        assert_eq!(form.method, "GET");
        assert!(form.action.starts_with("https://consent.google.com/save?"));
        assert!(form.action.contains("set_aps=true") && form.action.contains("set_sc=true"));
        assert!(
            form.action.contains("escs=ACCEPTTOKEN"),
            "picked accept, not reject"
        );
        assert!(!form.action.contains("set_eom=true"));
    }

    #[test]
    fn js_unescape_decodes_hex() {
        assert_eq!(js_unescape(r"a\x3db\x26c"), "a=b&c");
        assert_eq!(js_unescape("plain"), "plain");
    }

    #[test]
    fn google_hosts_get_socs() {
        assert_eq!(
            cookies_for_host("www.google.com"),
            &[("SOCS", "CAESHAgBEhIaAB")]
        );
        assert_eq!(
            cookies_for_host("google.co.uk"),
            &[("SOCS", "CAESHAgBEhIaAB")]
        );
        assert_eq!(
            cookies_for_host("www.youtube.com"),
            &[("SOCS", "CAESHAgBEhIaAB")]
        );
    }

    #[test]
    fn unknown_hosts_get_nothing() {
        assert!(cookies_for_host("example.com").is_empty());
        assert!(cookies_for_host("nike.com").is_empty());
    }

    // A faithful shape of google's consent interstitial: a Reject-all form and an
    // Accept-all form to `consent.google.com/save`, distinguished only by their
    // `set_aps`/`set_sc` selector values. The `escs` signed token rides in each form.
    const INTERSTITIAL: &str = r#"<!DOCTYPE html><html><body>
      <h1>Before you continue to Google</h1>
      <form method="POST" action="https://consent.google.com/save">
        <input type="hidden" name="gl" value="MT">
        <input type="hidden" name="hl" value="en">
        <input type="hidden" name="pc" value="shp">
        <input type="hidden" name="continue" value="https://www.google.com/search?q=cats&amp;hl=en">
        <input type="hidden" name="bl" value="gws_20260908">
        <input type="hidden" name="escs" value="ATu9is_REJECT_TOKEN">
        <input type="hidden" name="set_eom" value="true">
        <input type="hidden" name="set_aps" value="false">
        <input type="hidden" name="set_sc" value="false">
        <button type="submit">Reject all</button>
      </form>
      <form method="POST" action="https://consent.google.com/save">
        <input type="hidden" name="gl" value="MT">
        <input type="hidden" name="hl" value="en">
        <input type="hidden" name="pc" value="shp">
        <input type="hidden" name="continue" value="https://www.google.com/search?q=cats&amp;hl=en">
        <input type="hidden" name="bl" value="gws_20260908">
        <input type="hidden" name="escs" value="ATu9is_ACCEPT_TOKEN">
        <input type="hidden" name="set_eom" value="false">
        <input type="hidden" name="set_aps" value="true">
        <input type="hidden" name="set_sc" value="true">
        <button type="submit">Accept all</button>
      </form>
      </body></html>"#;

    #[test]
    fn detects_consent_interstitial() {
        assert!(is_consent_interstitial(
            "https://www.google.com/",
            INTERSTITIAL
        ));
        assert!(is_consent_interstitial(
            "https://consent.google.com/m?continue=x",
            ""
        ));
        assert!(!is_consent_interstitial(
            "https://www.google.com/",
            "<html><body>ordinary homepage</body></html>"
        ));
    }

    #[test]
    fn picks_accept_form_not_reject() {
        let form = parse_accept_form("https://www.google.com/", INTERSTITIAL).unwrap();
        assert_eq!(form.action, "https://consent.google.com/save");
        assert_eq!(form.method, "POST");
        // The ACCEPT token, not the reject one — we selected the right form.
        assert!(form
            .fields
            .contains(&("escs".to_string(), "ATu9is_ACCEPT_TOKEN".to_string())));
        assert!(form
            .fields
            .contains(&("set_aps".to_string(), "true".to_string())));
        assert!(form
            .fields
            .contains(&("set_sc".to_string(), "true".to_string())));
        // `&amp;` in the continue URL is unescaped.
        assert_eq!(
            continue_of(&form).as_deref(),
            Some("https://www.google.com/search?q=cats&hl=en")
        );
    }

    #[test]
    fn no_accept_form_is_none() {
        // Only a reject form → no accept form to submit.
        let reject_only = r#"<form method="POST" action="https://consent.google.com/save">
            <input name="set_aps" value="false"><input name="set_sc" value="false"></form>"#;
        assert!(parse_accept_form("https://www.google.com/", reject_only).is_none());
    }

    // A localhost server that plays google's consent + search flow:
    //  - GET /search WITHOUT NID  → serves the JS-gated `enablejs` shell (0 results).
    //  - GET /search WITH NID     → serves the real SERP (`<h3>` results, id="rso").
    //  - POST /save with set_aps=true&set_sc=true → Set-Cookie: NID (+SOCS) then 302 continue.
    //  - POST /save otherwise (reject) → no NID.
    async fn spawn_consent_fixture() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = req.lines().next().unwrap_or("");
                    let has_nid = req.contains("NID=");
                    let is_post = line.starts_with("POST");
                    // The accept selectors present in the POST body.
                    let accepts = req.contains("set_aps=true") && req.contains("set_sc=true");

                    let resp = if is_post && line.contains("/save") && accepts {
                        // Accept → mint NID (+SOCS) and bounce back to continue.
                        "HTTP/1.1 302 Found\r\nSet-Cookie: NID=511=trusted_session_value; Path=/; HttpOnly\r\nSet-Cookie: SOCS=CAI; Path=/\r\nLocation: /search?q=cats\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                    } else if is_post && line.contains("/save") {
                        // Reject → no NID.
                        http_ok("<html><body>rejected</body></html>")
                    } else if line.starts_with("GET /search") && has_nid {
                        // Trusted session → the real SERP.
                        http_ok(
                            r#"<!DOCTYPE html><html><body><div id="rso">
                            <a href="https://example.com/1"><h3>Real Result One</h3></a>
                            </div></body></html>"#,
                        )
                    } else if line.starts_with("GET /search") {
                        // No trusted session → the enablejs shell.
                        http_ok(
                            r#"<!DOCTYPE html><html><body>
                            <noscript>enablejs</noscript>
                            <a href="/httpservice/retry/enablejs">enable javascript</a>
                            </body></html>"#,
                        )
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

    // The full cookie-earning chain, offline: an interstitial whose Accept-all form
    // POSTs to a `/save` that mints NID → the jar gains NID → `/search` flips from the
    // enablejs shell to the real SERP.
    #[tokio::test]
    async fn accept_handshake_earns_nid_and_flips_search() {
        let (base, _srv) = spawn_consent_fixture().await;
        let mut jar = CookieJar::new();

        // The interstitial's accept form points its `/save` at our fixture, continuing
        // to the fixture's /search.
        let interstitial = format!(
            r#"<h1>Before you continue to Google</h1>
            <form method="POST" action="{base}/save">
              <input name="continue" value="{base}/search?q=cats">
              <input name="escs" value="ACCEPT_TOK">
              <input name="set_eom" value="false">
              <input name="set_aps" value="true">
              <input name="set_sc" value="true">
            </form>"#
        );

        // Before the handshake: /search is the enablejs shell.
        let pre = net::fetch_html(
            &format!("{base}/search?q=cats"),
            FetchOptions {
                jar: Some(&mut jar),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            pre.html.contains("enablejs"),
            "pre-handshake: enablejs shell"
        );
        assert!(!pre.html.contains("<h3"), "pre-handshake: no results");

        // Run the handshake.
        let earned = handshake_if_consent(&mut jar, &format!("{base}/"), &interstitial, "", 0.0)
            .await
            .unwrap()
            .expect("consent interstitial should be handled");
        assert!(earned.earned_nid, "handshake must mint NID, got {earned:?}");
        assert!(
            earned.minted.iter().any(|c| c == "NID"),
            "NID among minted cookies: {earned:?}"
        );

        // The jar now carries NID for the search endpoint.
        assert!(
            jar.cookies_for(&format!("{base}/search"), 0.0)
                .iter()
                .any(|c| c.name == NID_COOKIE),
            "jar must carry NID after the handshake"
        );

        // /search now returns the real SERP.
        let post = net::fetch_html(
            &format!("{base}/search?q=cats"),
            FetchOptions {
                jar: Some(&mut jar),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            post.html.contains("<h3") && post.html.contains("id=\"rso\""),
            "post-handshake /search must return the real SERP, got: {}",
            post.html
        );
        assert!(!post.html.contains("enablejs"), "no more enablejs shell");
    }

    // A reject-only interstitial must NOT mint NID (and the handshake errors rather
    // than silently POSTing the wrong form).
    #[tokio::test]
    async fn reject_only_earns_no_nid() {
        let (base, _srv) = spawn_consent_fixture().await;
        let mut jar = CookieJar::new();
        let reject_only = format!(
            r#"<h1>Before you continue to Google</h1>
            <form method="POST" action="{base}/save">
              <input name="continue" value="{base}/search?q=cats">
              <input name="set_aps" value="false">
              <input name="set_sc" value="false">
            </form>"#
        );
        let out = handshake_if_consent(&mut jar, &format!("{base}/"), &reject_only, "", 0.0).await;
        assert!(out.is_err(), "no accept form → clear error, got {out:?}");
        assert!(
            jar.cookies_for(&format!("{base}/search"), 0.0)
                .iter()
                .all(|c| c.name != NID_COOKIE),
            "no NID may be minted without submitting the accept form"
        );
    }

    // Non-consent pages are a no-op (Ok(None)).
    #[tokio::test]
    async fn ordinary_page_is_noop() {
        let mut jar = CookieJar::new();
        let out = handshake_if_consent(
            &mut jar,
            "https://www.google.com/",
            "<html><body>homepage</body></html>",
            "",
            0.0,
        )
        .await
        .unwrap();
        assert!(out.is_none(), "ordinary page → nothing to do");
    }
}
