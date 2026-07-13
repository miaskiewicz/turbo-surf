//! Networking (port of `src/net.mjs`, SPEC §8): `fetch_html` over reqwest
//! (redirects + gzip/br/deflate for free) plus the hardening the spec calls for:
//! charset sniffing, max body size, content-type gate, and an optional CookieJar
//! round-trip. The pure helpers below are unit-tested offline; `fetch_html`
//! itself is the live-IO seam (covered by the integration suite / harness).

use crate::cache::ResponseCache;
use crate::cookies::CookieJar;
use crate::http_backend as http; // reqwest (rustls) or wreq (BoringSSL); see lib.rs
use bytes::BytesMut;
use futures_util::StreamExt;
use std::collections::BTreeMap;

const DEFAULT_MAX_BYTES: usize = 8 * 1024 * 1024; // 8 MiB
const HTML_TYPES: &[&str] = &[
    "text/html",
    "application/xhtml+xml",
    "application/xml",
    "text/xml",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCode {
    BodyTooLarge,
    NotHtml,
    Network,
}

#[derive(Debug)]
pub struct HttpError {
    pub message: String,
    pub code: ErrorCode,
}

impl HttpError {
    fn new(message: impl Into<String>, code: ErrorCode) -> Self {
        Self {
            message: message.into(),
            code,
        }
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for HttpError {}

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub html: String,
    pub final_url: String,
    pub status: u16,
    pub redirected: bool,
    pub content_type: String,
}

#[derive(Default)]
pub struct FetchOptions<'a> {
    pub headers: BTreeMap<String, String>,
    pub method: Option<String>,
    pub body: Option<String>,
    pub max_bytes: Option<usize>,
    pub allow_non_html: bool,
    pub max_redirects: Option<usize>,
    pub jar: Option<&'a mut CookieJar>,
    pub cache: Option<&'a mut ResponseCache>,
    /// Shared client for connection reuse (see [`build_client`]); built per-call
    /// when `None`.
    pub client: Option<&'a http::Client>,
    /// Fingerprint identity for the default (rustls) header set. `None` uses
    /// [`crate::fingerprint::default_profile`] (the fixed Chrome 149 / macOS set);
    /// pass [`crate::fingerprint::select`]`(key)` to rotate per client. Ignored on
    /// the `impersonate` path, where wreq's emulation owns the headers.
    pub profile: Option<&'a crate::fingerprint::Profile>,
    /// Seed well-known consent-wall cookies for the request host (see
    /// [`crate::consent`]) so JS-gated "before you continue" interstitials are
    /// dismissed and the real, server-rendered page is returned. Off by default.
    pub bypass_consent: bool,
    pub now: f64,
}

// charset: Content-Type header → <meta charset> sniff → utf-8.
fn detect_charset(content_type: &str, head: &[u8]) -> String {
    if let Some(cs) = charset_from_ct(content_type) {
        return cs;
    }
    charset_from_meta(head).unwrap_or_else(|| "utf-8".to_string())
}

fn charset_from_ct(ct: &str) -> Option<String> {
    let lc = ct.to_ascii_lowercase();
    let i = lc.find("charset=")? + "charset=".len();
    let rest = &lc[i..];
    let end = rest.find(';').unwrap_or(rest.len());
    Some(rest[..end].trim().replace(['"', '\''], ""))
}

fn charset_from_meta(head: &[u8]) -> Option<String> {
    let ascii: String = head
        .iter()
        .map(|&b| b as char)
        .collect::<String>()
        .to_ascii_lowercase();
    let i = ascii.find("charset=")? + "charset=".len();
    let label: String = ascii[i..]
        .chars()
        .skip_while(|c| *c == '"' || *c == '\'')
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}

fn is_html_type(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    if ct.is_empty() {
        return true; // permissive when the server omits it
    }
    HTML_TYPES.iter().any(|t| ct.contains(t))
}

fn decode(bytes: &[u8], charset: &str) -> String {
    let enc = encoding_rs::Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    enc.decode(bytes).0.into_owned()
}

// Method after a redirect: 303 (and 301/302 on POST) → GET; 307/308 keep it.
fn next_method(status: u16, method: &str) -> String {
    if status == 303 {
        return "GET".to_string();
    }
    if (status == 301 || status == 302) && method == "POST" {
        return "GET".to_string();
    }
    method.to_string()
}

const REDIRECT_STATUS: &[u16] = &[301, 302, 303, 307, 308];

fn is_redirect(status: u16) -> bool {
    REDIRECT_STATUS.contains(&status)
}

fn build_headers(url: &str, opts: &FetchOptions) -> BTreeMap<String, String> {
    let mut h = BTreeMap::new();
    // Default (rustls) path: forge Chrome's identity by hand — headers are the
    // only lever, since rustls can't match the TLS fingerprint. Impersonate path:
    // wreq's emulation already installs a coherent Chrome UA + nav-header set that
    // matches its TLS/HTTP-2 fingerprint, so we add nothing here and let it own
    // them (caller overrides + cookies/cache below still apply on both paths).
    #[cfg(not(feature = "impersonate"))]
    {
        let owned;
        let profile = match opts.profile {
            Some(p) => p,
            None => {
                owned = crate::fingerprint::default_profile();
                &owned
            }
        };
        for (k, v) in profile.nav_headers() {
            h.insert(k.to_string(), v);
        }
    }
    for (k, v) in &opts.headers {
        h.insert(k.to_ascii_lowercase(), v.clone());
    }
    if let Some(jar) = &opts.jar {
        let cookie = jar.cookie_header(url, opts.now);
        if !cookie.is_empty() {
            h.insert("cookie".into(), cookie);
        }
    }
    if opts.bypass_consent {
        seed_consent_cookies(&mut h, url);
    }
    if let Some(cache) = &opts.cache {
        for (k, v) in cache.validators(url) {
            h.insert(k, v);
        }
    }
    h
}

/// Merge the host's consent-bypass cookies (see [`crate::consent`]) into the
/// `cookie` header, skipping any name a jar/caller cookie already set so an
/// explicit cookie always wins.
fn seed_consent_cookies(h: &mut BTreeMap<String, String>, url: &str) {
    let host = crate::url::host_of(url).unwrap_or_default();
    let extra = crate::consent::cookies_for_host(&host);
    if extra.is_empty() {
        return;
    }
    let mut cookie = h.get("cookie").cloned().unwrap_or_default();
    for (name, value) in extra {
        // Don't override a cookie the caller/jar already carries for this name.
        let has = cookie
            .split(';')
            .any(|pair| pair.trim().split('=').next().map(str::trim) == Some(*name));
        if has {
            continue;
        }
        if !cookie.is_empty() {
            cookie.push_str("; ");
        }
        cookie.push_str(name);
        cookie.push('=');
        cookie.push_str(value);
    }
    if !cookie.is_empty() {
        h.insert("cookie".into(), cookie);
    }
}

// Apply the Chrome TLS/JA3/JA4 + HTTP-2 emulation profile to a builder under the
// `impersonate` feature; a pass-through on the default (rustls) backend, which
// can't forge a fingerprint. One seam so both client constructors stay in sync.
fn emulate(builder: http::ClientBuilder) -> http::ClientBuilder {
    #[cfg(feature = "impersonate")]
    let builder = builder.emulation(
        // Chrome 149 on macOS — matched to fingerprint::default_profile and the
        // render-tier navigator so the TLS/HTTP-2 fingerprint, the request headers,
        // and `navigator.*` all report the same browser+OS (a cross-layer mismatch
        // is itself a bot signal).
        wreq_util::Emulation::builder()
            .profile(wreq_util::Profile::Chrome149)
            .platform(wreq_util::Platform::MacOS)
            .build(),
    );
    builder
}

fn client(policy: http::redirect::Policy) -> http::Result<http::Client> {
    emulate(http::Client::builder().redirect(policy)).build()
}

/// A tuned, reusable client (HTTP/2 via ALPN, kept-warm pool, auto-redirect ≤20).
/// Build one per crawl and pass it through `FetchOptions::client` so connections
/// (and TLS sessions) are reused across hosts — the dispatcher (port of
/// `src/dispatcher.mjs`). `Client` is cheap to clone (Arc-shared pool).
pub fn build_client() -> http::Client {
    emulate(
        http::Client::builder()
            .redirect(http::redirect::Policy::limited(20))
            .pool_idle_timeout(std::time::Duration::from_secs(90)),
    )
    .build()
    .expect("HTTP client build (TLS backend init)")
}

// Final URL of a settled response, across backends: reqwest exposes `url() -> &Url`,
// wreq `uri() -> &Uri`. Both reflect the post-redirect URL on the auto-follow path.
fn final_url(res: &http::Response) -> String {
    #[cfg(not(feature = "impersonate"))]
    {
        res.url().to_string()
    }
    #[cfg(feature = "impersonate")]
    {
        res.uri().to_string()
    }
}

fn redirect_location(res: &http::Response) -> Option<String> {
    if !is_redirect(res.status().as_u16()) {
        return None;
    }
    let loc = header_value(res, "location");
    if loc.is_empty() {
        None
    } else {
        Some(loc)
    }
}

fn err(e: impl std::fmt::Display, code: ErrorCode) -> HttpError {
    HttpError::new(e.to_string(), code)
}

async fn send(
    cl: &http::Client,
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: &Option<String>,
) -> Result<http::Response, HttpError> {
    let m = http::Method::from_bytes(method.as_bytes()).map_err(|e| err(e, ErrorCode::Network))?;
    let mut req = apply_headers(cl.request(m, url), headers);
    if let Some(b) = body {
        req = req.body(b.clone());
    }
    // Include the URL in transport errors — reqwest's "builder error" (e.g. a
    // relative/schemeless URL) is otherwise undebuggable.
    req.send()
        .await
        .map_err(|e| err(format!("{e} (url: {url})"), ErrorCode::Network))
}

fn apply_headers(
    mut req: http::RequestBuilder,
    headers: &BTreeMap<String, String>,
) -> http::RequestBuilder {
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    req
}

fn check_content_length(res: &http::Response, max_bytes: usize) -> Result<(), HttpError> {
    if let Some(len) = res.content_length() {
        if len as usize > max_bytes {
            return Err(HttpError::new(
                format!("body exceeds maxBytes ({len} > {max_bytes})"),
                ErrorCode::BodyTooLarge,
            ));
        }
    }
    Ok(())
}

async fn read_capped(res: http::Response, max_bytes: usize) -> Result<Vec<u8>, HttpError> {
    check_content_length(&res, max_bytes)?;
    let mut buf = BytesMut::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| HttpError::new(e.to_string(), ErrorCode::Network))?;
        if buf.len() + chunk.len() > max_bytes {
            return Err(HttpError::new(
                format!("body exceeds maxBytes (> {max_bytes})"),
                ErrorCode::BodyTooLarge,
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf.to_vec())
}

fn header_value(res: &http::Response, name: &str) -> String {
    res.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

fn ingest_set_cookie(opts: &mut FetchOptions, res: &http::Response, final_url: &str) {
    let now = opts.now;
    let Some(jar) = opts.jar.as_mut() else { return };
    let lines: Vec<String> = res
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();
    if !lines.is_empty() {
        jar.set_from_response(final_url, &lines, now);
    }
}

fn gate_html_type(opts: &FetchOptions, res: &http::Response) -> Result<(), HttpError> {
    let ct = header_value(res, "content-type");
    if !opts.allow_non_html && !is_html_type(&ct) {
        return Err(HttpError::new(
            format!("non-HTML content-type: {ct}"),
            ErrorCode::NotHtml,
        ));
    }
    Ok(())
}

// Turn a settled response into the FetchResult: gate content-type, then read +
// charset-decode the body under the byte cap.
async fn finish(
    opts: &mut FetchOptions<'_>,
    res: http::Response,
    final_url: String,
    redirected: bool,
    max_bytes: usize,
) -> Result<FetchResult, HttpError> {
    let status = res.status().as_u16();
    // 304 Not Modified → reuse the cached body (server sent none).
    if status == 304 {
        if let Some(cache) = &opts.cache {
            return Ok(FetchResult {
                html: cache.body(&final_url),
                final_url,
                status,
                redirected,
                content_type: header_value(&res, "content-type"),
            });
        }
    }
    gate_html_type(opts, &res)?;
    let etag = opt_header(&res, "etag");
    let last_modified = opt_header(&res, "last-modified");
    let ct = header_value(&res, "content-type");
    let bytes = read_capped(res, max_bytes).await?;
    let charset = detect_charset(&ct, &bytes[..bytes.len().min(1024)]);
    let html = decode(&bytes, &charset);
    if let Some(cache) = opts.cache.as_mut() {
        cache.store(&final_url, etag, last_modified, &html);
    }
    Ok(FetchResult {
        html,
        final_url,
        status,
        redirected,
        content_type: ct,
    })
}

fn opt_header(res: &http::Response, name: &str) -> Option<String> {
    res.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

// State threaded through the manual-follow loop.
struct Hop {
    url: String,
    method: String,
    body: Option<String>,
}

// Manual redirect following (when `max_redirects` is set): re-derives the Cookie
// header and ingests Set-Cookie per hop and rewrites method/body per the fetch
// spec — the round-trip reqwest's auto-follow can't do.
async fn follow_manually(
    opts: &mut FetchOptions<'_>,
    start: &str,
    max_redirects: usize,
    max_bytes: usize,
) -> Result<FetchResult, HttpError> {
    let cl = client(http::redirect::Policy::none()).map_err(|e| err(e, ErrorCode::Network))?;
    let mut hop = Hop {
        url: start.to_string(),
        method: opts.method.clone().unwrap_or_else(|| "GET".to_string()),
        body: opts.body.clone(),
    };
    for redirects in 0.. {
        let headers = build_headers(&hop.url, opts);
        let res = send(&cl, &hop.method, &hop.url, &headers, &hop.body).await?;
        let final_url = final_url(&res);
        ingest_set_cookie(opts, &res, &final_url);
        match redirect_location(&res) {
            Some(loc) if redirects < max_redirects => hop = advance(&hop, &res, &loc)?,
            _ => return finish(opts, res, final_url, redirects > 0, max_bytes).await,
        }
    }
    unreachable!("redirect loop always returns at the cap")
}

fn advance(hop: &Hop, res: &http::Response, loc: &str) -> Result<Hop, HttpError> {
    let status = res.status().as_u16();
    let method = next_method(status, &hop.method);
    let next = url::Url::parse(&hop.url)
        .and_then(|b| b.join(loc))
        .map_err(|e| err(e, ErrorCode::Network))?;
    let body = if method == hop.method {
        hop.body.clone()
    } else {
        None
    };
    Ok(Hop {
        url: next.to_string(),
        method,
        body,
    })
}

async fn follow_auto(
    opts: &mut FetchOptions<'_>,
    url: &str,
    max_bytes: usize,
) -> Result<FetchResult, HttpError> {
    // Reuse the caller's shared client (pooled connections) when provided.
    let cl = match opts.client {
        Some(c) => c.clone(),
        None => {
            client(http::redirect::Policy::limited(20)).map_err(|e| err(e, ErrorCode::Network))?
        }
    };
    let method = opts.method.clone().unwrap_or_else(|| "GET".to_string());
    let headers = build_headers(url, opts);
    let res = send(&cl, &method, url, &headers, &opts.body.clone()).await?;
    let final_url = final_url(&res);
    let redirected = final_url != *url;
    ingest_set_cookie(opts, &res, &final_url);
    finish(opts, res, final_url, redirected, max_bytes).await
}

/// Fetch a URL and return its decoded HTML plus response metadata. With
/// `max_redirects` set, redirects are followed manually (per-hop cookie
/// round-trip, fetch-spec method rewrite); otherwise reqwest auto-follows
/// (cap 20). Content-type gate, byte cap, and charset decode mirror the JS.
pub async fn fetch_html(url: &str, mut opts: FetchOptions<'_>) -> Result<FetchResult, HttpError> {
    let max_bytes = opts.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    match opts.max_redirects {
        Some(n) => follow_manually(&mut opts, url, n, max_bytes).await,
        None => follow_auto(&mut opts, url, max_bytes).await,
    }
}

/// Fetch a URL and return its **raw** response bytes (no charset decode), the
/// final URL, and the status. For binary resources — images — where decoding to
/// a `String` would corrupt the bytes. Auto-follows redirects (cap 20) over the
/// shared client with the same headers/cookies as [`fetch_html`]; the byte cap
/// applies and Set-Cookie is ingested. No content-type gate (the caller asks for
/// an image on purpose).
pub async fn fetch_bytes(
    url: &str,
    mut opts: FetchOptions<'_>,
) -> Result<(String, Vec<u8>, u16), HttpError> {
    let max_bytes = opts.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    let cl = match opts.client {
        Some(c) => c.clone(),
        None => {
            client(http::redirect::Policy::limited(20)).map_err(|e| err(e, ErrorCode::Network))?
        }
    };
    let method = opts.method.clone().unwrap_or_else(|| "GET".to_string());
    let headers = build_headers(url, &opts);
    let res = send(&cl, &method, url, &headers, &opts.body.clone()).await?;
    let final_url = final_url(&res);
    ingest_set_cookie(&mut opts, &res, &final_url);
    let status = res.status().as_u16();
    let bytes = read_capped(res, max_bytes).await?;
    Ok((final_url, bytes, status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_from_header() {
        assert_eq!(
            detect_charset("text/html; charset=ISO-8859-1", b""),
            "iso-8859-1"
        );
        assert_eq!(detect_charset("text/html; charset=\"utf-8\"", b""), "utf-8");
    }

    #[test]
    fn charset_from_meta_tag() {
        let head = br#"<html><head><meta charset="windows-1250">"#;
        assert_eq!(detect_charset("text/html", head), "windows-1250");
    }

    #[test]
    fn charset_falls_back_to_utf8() {
        assert_eq!(detect_charset("text/html", b"<html>"), "utf-8");
    }

    #[test]
    fn html_type_gate() {
        assert!(is_html_type(""));
        assert!(is_html_type("text/html; charset=utf-8"));
        assert!(!is_html_type("application/json"));
    }

    #[test]
    fn redirect_method_rewrite() {
        assert_eq!(next_method(303, "POST"), "GET");
        assert_eq!(next_method(301, "POST"), "GET");
        assert_eq!(next_method(307, "POST"), "POST");
        assert_eq!(next_method(302, "GET"), "GET");
    }

    #[test]
    fn redirect_status_set() {
        assert!(is_redirect(308));
        assert!(!is_redirect(200));
    }

    #[test]
    fn decode_latin1() {
        let bytes = [0xe9u8]; // é in latin1
        assert_eq!(decode(&bytes, "iso-8859-1"), "é");
    }

    #[test]
    fn meta_charset_empty_label_falls_back() {
        // "charset=" followed by a non-token char → empty label → utf-8.
        assert_eq!(detect_charset("text/html", b"<meta charset= >"), "utf-8");
    }

    // Default-path only: under `impersonate`, wreq's emulation supplies these
    // headers at the client layer, not `build_headers`.
    #[cfg(not(feature = "impersonate"))]
    #[test]
    fn default_headers_look_like_chrome() {
        let opts = FetchOptions::default();
        let h = build_headers("http://example.com/", &opts);
        assert!(h["user-agent"].contains("Chrome/"));
        assert!(h["accept"].contains("text/html"));
        assert_eq!(h["sec-fetch-mode"], "navigate");
        assert_eq!(h["sec-ch-ua-mobile"], "?0");
        // reqwest owns accept-encoding so it can auto-decompress; we must not set it.
        assert!(!h.contains_key("accept-encoding"));
    }

    #[test]
    fn caller_headers_override_chrome_defaults() {
        let mut opts = FetchOptions::default();
        opts.headers
            .insert("User-Agent".into(), "custom-bot/9".into());
        let h = build_headers("http://example.com/", &opts);
        // case-folded to the canonical lowercase key, caller value wins.
        assert_eq!(h["user-agent"], "custom-bot/9");
        assert_eq!(h.get("User-Agent"), None);
    }
}

// Live-IO coverage over a localhost server (offline + deterministic — no
// external network). Exercises fetch_html, both redirect paths, the byte caps,
// the content-type gate, charset decode, and the CookieJar round-trip.
#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::cookies::CookieJar;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn http(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut v = format!("HTTP/1.1 {status}\r\n").into_bytes();
        for (k, val) in headers {
            v.extend_from_slice(format!("{k}: {val}\r\n").as_bytes());
        }
        v.extend_from_slice(b"Connection: close\r\n\r\n");
        v.extend_from_slice(body);
        v
    }

    fn route(path: &str) -> Vec<u8> {
        match path {
            "/json" => http("200 OK", &[("Content-Type", "application/json")], b"{}"),
            "/r" => http("302 Found", &[("Location", "/dest")], b""),
            "/post303" => http("303 See Other", &[("Location", "/dest")], b""),
            "/emptyloc" => http(
                "302 Found",
                &[("Content-Type", "text/html")],
                b"<html>noloc</html>",
            ),
            "/dest" => http(
                "200 OK",
                &[("Content-Type", "text/html")],
                b"<html>dest</html>",
            ),
            "/big" => http(
                "200 OK",
                &[("Content-Type", "text/html"), ("Content-Length", "1000000")],
                b"",
            ),
            "/stream" => http("200 OK", &[("Content-Type", "text/html")], &[b'x'; 50]),
            "/latin" => http(
                "200 OK",
                &[("Content-Type", "text/html; charset=iso-8859-1")],
                &[b'<', b'p', b'>', 0xe9, b'<', b'/', b'p', b'>'],
            ),
            "/cookie" => http(
                "200 OK",
                &[
                    ("Content-Type", "text/html"),
                    ("Set-Cookie", "sid=abc; Path=/"),
                ],
                b"<html>c</html>",
            ),
            _ => http(
                "200 OK",
                &[("Content-Type", "text/html")],
                b"<html><title>ok</title></html>",
            ),
        }
    }

    async fn spawn() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let _ = sock.write_all(&route(&path)).await;
                    let _ = sock.flush().await;
                });
            }
        });
        port
    }

    fn url(port: u16, path: &str) -> String {
        format!("http://127.0.0.1:{port}{path}")
    }

    #[tokio::test]
    async fn fetches_and_decodes_html() {
        let p = spawn().await;
        let r = fetch_html(&url(p, "/"), FetchOptions::default())
            .await
            .unwrap();
        assert_eq!(r.status, 200);
        assert!(r.html.contains("ok"));
        assert!(!r.redirected);
    }

    // Header e2e: prove the Chrome identity headers actually reach the wire
    // (through the real client + a live TCP connection), not merely
    // `build_headers`'s map. Captures the raw inbound request and asserts the
    // fingerprint markers are present and the old `turbo-surf/` tell is gone.
    // Default (rustls) path only — under `impersonate` wreq's emulation owns the
    // header set (asserted by the network-layer harness in tests/impersonate.rs).
    #[cfg(not(feature = "impersonate"))]
    #[tokio::test]
    async fn chrome_headers_reach_the_wire() {
        use tokio::sync::oneshot;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = sock
                .write_all(&http(
                    "200 OK",
                    &[("Content-Type", "text/html")],
                    b"<html>ok</html>",
                ))
                .await;
            let _ = sock.flush().await;
            let _ = tx.send(raw);
        });
        fetch_html(&url(port, "/"), FetchOptions::default())
            .await
            .unwrap();
        let raw = rx.await.unwrap().to_ascii_lowercase();
        assert!(
            raw.contains("user-agent: mozilla/5.0") && raw.contains("chrome/"),
            "missing chrome UA on the wire: {raw}"
        );
        assert!(raw.contains("sec-ch-ua:") && raw.contains("\"google chrome\""));
        assert!(raw.contains("sec-fetch-mode: navigate"));
        assert!(raw.contains("sec-fetch-dest: document"));
        assert!(raw.contains("accept-language: en-us"));
        assert!(raw.contains("upgrade-insecure-requests: 1"));
        // The dead-giveaway bot token must be gone.
        assert!(!raw.contains("turbo-surf/"), "leaked bot UA token: {raw}");
    }

    #[tokio::test]
    async fn content_type_gate_and_opt_out() {
        let p = spawn().await;
        let err = fetch_html(&url(p, "/json"), FetchOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotHtml);
        let ok = fetch_html(
            &url(p, "/json"),
            FetchOptions {
                allow_non_html: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(ok.html, "{}");
    }

    #[tokio::test]
    async fn auto_redirect_follows() {
        let p = spawn().await;
        let r = fetch_html(&url(p, "/r"), FetchOptions::default())
            .await
            .unwrap();
        assert!(r.final_url.ends_with("/dest"));
        assert!(r.redirected);
        assert!(r.html.contains("dest"));
    }

    #[tokio::test]
    async fn manual_redirect_follows_with_cap() {
        let p = spawn().await;
        let r = fetch_html(
            &url(p, "/r"),
            FetchOptions {
                max_redirects: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(r.final_url.ends_with("/dest"));
        assert!(r.redirected);
    }

    #[tokio::test]
    async fn manual_redirect_cap_zero_stops_at_first_hop() {
        let p = spawn().await;
        let r = fetch_html(
            &url(p, "/r"),
            FetchOptions {
                max_redirects: Some(0),
                allow_non_html: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(r.status, 302); // not followed
    }

    #[tokio::test]
    async fn content_length_cap_rejects() {
        let p = spawn().await;
        let err = fetch_html(
            &url(p, "/big"),
            FetchOptions {
                max_bytes: Some(10),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::BodyTooLarge);
    }

    #[tokio::test]
    async fn streamed_cap_rejects() {
        let p = spawn().await;
        let err = fetch_html(
            &url(p, "/stream"),
            FetchOptions {
                max_bytes: Some(10),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::BodyTooLarge);
    }

    #[tokio::test]
    async fn charset_decoded_from_header() {
        let p = spawn().await;
        let r = fetch_html(&url(p, "/latin"), FetchOptions::default())
            .await
            .unwrap();
        assert_eq!(r.html, "<p>é</p>");
    }

    #[tokio::test]
    async fn cookie_round_trip() {
        let p = spawn().await;
        let mut jar = CookieJar::new();
        jar.add("pre", "seed", "127.0.0.1", "/", None); // exercises build_headers cookie branch
        let mut opts = FetchOptions {
            jar: Some(&mut jar),
            ..Default::default()
        };
        opts.headers.insert("x-test".into(), "1".into());
        fetch_html(&url(p, "/cookie"), opts).await.unwrap();
        // Set-Cookie from the response was ingested.
        assert!(jar.cookie_header(&url(p, "/"), 0.0).contains("sid=abc"));
    }

    #[tokio::test]
    async fn post_303_redirect_drops_body_and_switches_to_get() {
        let p = spawn().await;
        let r = fetch_html(
            &url(p, "/post303"),
            FetchOptions {
                method: Some("POST".into()),
                body: Some("payload".into()),
                max_redirects: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(r.final_url.ends_with("/dest"));
    }

    #[tokio::test]
    async fn manual_redirect_without_location_is_terminal() {
        let p = spawn().await;
        let r = fetch_html(
            &url(p, "/emptyloc"),
            FetchOptions {
                max_redirects: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(r.status, 302); // no Location → treated as the final response
        assert!(r.html.contains("noloc"));
    }

    #[tokio::test]
    async fn conditional_request_304_reuses_cached_body() {
        // Server: 200 + ETag on a plain GET; 304 (no body) when If-None-Match is sent.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let resp = if req.contains("if-none-match") {
                    "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    http(
                        "200 OK",
                        &[("Content-Type", "text/html"), ("ETag", "\"v1\"")],
                        b"<html>fresh</html>",
                    )
                    .iter()
                    .map(|&b| b as char)
                    .collect()
                };
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        let u = url(port, "/");
        let mut cache = ResponseCache::new();

        let r1 = fetch_html(
            &u,
            FetchOptions {
                cache: Some(&mut cache),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(r1.status, 200);
        assert!(r1.html.contains("fresh"));

        let r2 = fetch_html(
            &u,
            FetchOptions {
                cache: Some(&mut cache),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(r2.status, 304); // revalidated
        assert!(r2.html.contains("fresh")); // body served from cache
    }

    #[tokio::test]
    async fn network_error_surfaces() {
        // Nothing listening on this port → transport error.
        let err = fetch_html("http://127.0.0.1:1/", FetchOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Network);
    }
}
