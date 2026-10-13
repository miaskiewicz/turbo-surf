//! CookieJar (port of `src/cookies.mjs`, SPEC §8): RFC 6265 subset —
//! domain/path scoping, Secure, HttpOnly, Expires/Max-Age, SameSite. Times are
//! epoch milliseconds as `f64` so a session cookie is `f64::INFINITY`, matching
//! the JS `Infinity` sentinel exactly.

use std::collections::HashMap;
use url::Url;

#[derive(Clone, Debug)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: String,
    pub expires_at: f64,
}

// Parse-time scratch: attributes accumulate before expiry is resolved.
struct Parsed {
    name: String,
    value: String,
    domain: Option<String>,
    path: String,
    secure: bool,
    http_only: bool,
    same_site: String,
    max_age: Option<f64>,
    expires: Option<f64>,
}

fn apply_attr(c: &mut Parsed, attr: &str) {
    let (k, v) = split_attr(attr);
    match k.as_str() {
        "domain" => c.domain = Some(v.trim_start_matches('.').to_ascii_lowercase()),
        "path" => c.path = if v.is_empty() { "/".into() } else { v },
        "secure" => c.secure = true,
        "httponly" => c.http_only = true,
        "samesite" => c.same_site = v.to_ascii_lowercase(),
        "max-age" => c.max_age = parse_num(&v),
        "expires" => c.expires = parse_http_date(&v),
        _ => {}
    }
}

fn split_attr(attr: &str) -> (String, String) {
    match attr.find('=') {
        Some(i) => (
            attr[..i].trim().to_ascii_lowercase(),
            attr[i + 1..].trim().to_string(),
        ),
        None => (attr.trim().to_ascii_lowercase(), String::new()),
    }
}

fn parse_num(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok()
}

// Best-effort HTTP-date → epoch ms. Mirrors `Date.parse` for the formats robots
// hosts actually emit (RFC 1123 / RFC 850 / asctime via httpdate).
fn parse_http_date(v: &str) -> Option<f64> {
    httpdate_ms(v)
}

fn parse_set_cookie(line: &str) -> Option<Cookie> {
    let mut parts = line.split(';');
    let name_part = parts.next()?;
    let eq = name_part.find('=')?;
    let name = name_part[..eq].trim().to_string();
    let value = name_part[eq + 1..].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let mut c = Parsed {
        name,
        value,
        domain: None,
        path: "/".into(),
        secure: false,
        http_only: false,
        same_site: "lax".into(),
        max_age: None,
        expires: None,
    };
    for attr in parts {
        apply_attr(&mut c, attr);
    }
    Some(finalize(c))
}

// Project parsed attributes into a Cookie. `expires_at` is a placeholder here;
// the jar resolves it against `now` at ingest (Max-Age wins over Expires;
// neither → session cookie / Infinity). Domain default is filled at ingest too.
fn finalize(c: Parsed) -> Cookie {
    Cookie {
        name: c.name,
        value: c.value,
        host_only: c.domain.as_deref().is_none_or(str::is_empty),
        domain: c.domain.unwrap_or_default(),
        path: c.path,
        secure: c.secure,
        http_only: c.http_only,
        same_site: c.same_site,
        expires_at: f64::NAN,
    }
}

fn expiry_of(max_age: Option<f64>, expires: Option<f64>, now: f64) -> f64 {
    if let Some(m) = max_age {
        return if m <= 0.0 { 0.0 } else { now + m * 1000.0 };
    }
    expires.unwrap_or(f64::INFINITY)
}

fn domain_match(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn path_match(req_path: &str, cookie_path: &str) -> bool {
    if req_path == cookie_path {
        return true;
    }
    if req_path.starts_with(cookie_path) {
        return cookie_path.ends_with('/')
            || req_path.as_bytes().get(cookie_path.len()) == Some(&b'/');
    }
    false
}

struct ReqCtx {
    host: String,
    is_secure: bool,
    path: String,
    now: f64,
}

fn cookie_applies(c: &Cookie, ctx: &ReqCtx) -> bool {
    c.expires_at > ctx.now
        && (!c.secure || ctx.is_secure)
        && if c.host_only {
            ctx.host == c.domain
        } else {
            domain_match(&ctx.host, &c.domain)
        }
        && path_match(&ctx.path, &c.path)
}

fn default_cookie_path(url: &Url) -> &str {
    let path = url.path();
    match path.rfind('/') {
        Some(index) if index > 0 => &path[..index],
        _ => "/",
    }
}

fn explicit_cookie_path(line: &str) -> Option<String> {
    line.split(';')
        .skip(1)
        .filter_map(|attribute| {
            let (name, value) = split_attr(attribute);
            (name == "path").then_some(value)
        })
        .last()
}

fn secure_cookie_url(url: &Url) -> bool {
    url.scheme() == "https"
        || url.host_str().is_some_and(|host| {
            host == "localhost"
                || host.ends_with(".localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
}

fn rejected(c: &Cookie, url: &Url, line: &str) -> bool {
    let host = url.host_str().unwrap_or("");
    if !domain_match(host, &c.domain) || c.secure && !secure_cookie_url(url) {
        return true;
    }
    if !c.host_only && psl::suffix_str(&c.domain) == Some(c.domain.as_str()) && host != c.domain {
        return true;
    }
    if c.name.starts_with("__Secure-") && !c.secure {
        return true;
    }
    if c.name.starts_with("__Host-")
        && (!c.secure || !c.host_only || explicit_cookie_path(line).as_deref() != Some("/"))
    {
        return true;
    }
    if (c.name.starts_with("__Http-") || c.name.starts_with("__Host-Http-"))
        && (!c.secure || !c.http_only)
    {
        return true;
    }
    c.same_site == "none" && !c.secure
}

#[derive(Default)]
pub struct CookieJar {
    store: HashMap<String, Cookie>,
    request_changes: Option<HashMap<String, Option<Cookie>>>,
}

fn key(domain: &str, path: &str, name: &str) -> String {
    format!("{domain} {path} {name}")
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot request cookies without borrowing the shared jar across an await.
    /// Track writes, including unchanged values and deletions, rather than a final
    /// map diff: a response can intentionally overwrite a concurrent script write.
    pub fn fork_for_request(&self) -> Self {
        Self {
            store: self.store.clone(),
            request_changes: Some(HashMap::new()),
        }
    }

    /// Apply only writes made to a [`Self::fork_for_request`] snapshot. Unrelated
    /// cookies added by other requests or document.cookie remain intact.
    pub fn merge_request_changes(&mut self, request: Self) {
        if let Some(changes) = request.request_changes {
            for (key, cookie) in changes {
                self.write_cookie(key, cookie);
            }
        }
    }

    fn write_cookie(&mut self, key: String, cookie: Option<Cookie>) {
        if let Some(changes) = &mut self.request_changes {
            changes.insert(key.clone(), cookie.clone());
        }
        if let Some(cookie) = cookie {
            self.store.insert(key, cookie);
        } else {
            self.store.remove(&key);
        }
    }

    /// Ingest `Set-Cookie` header lines for the response URL.
    pub fn set_from_response(&mut self, url: &str, lines: &[String], now: f64) {
        let Ok(u) = Url::parse(url) else { return };
        for line in lines {
            self.ingest_one(line, &u, now);
        }
    }

    fn ingest_one(&mut self, line: &str, url: &Url, now: f64) {
        let host = url.host_str().unwrap_or("");
        let Some((raw, max_age, expires)) = parse_with_raw(line) else {
            return;
        };
        let domain = if raw.domain.is_empty() {
            host.to_string()
        } else {
            raw.domain.clone()
        };
        let mut c = raw;
        c.domain = domain.clone();
        if rejected(&c, url, line) {
            return;
        }
        if !c.host_only && psl::suffix_str(&domain) == Some(domain.as_str()) {
            c.host_only = true;
        }
        c.path = explicit_cookie_path(line)
            .filter(|path| path.starts_with('/'))
            .unwrap_or_else(|| default_cookie_path(url).to_owned());
        c.expires_at = expiry_of(max_age, expires, now);
        let k = key(&domain, &c.path, &c.name);
        if c.expires_at <= now {
            self.write_cookie(k, None);
        } else {
            self.write_cookie(k, Some(c));
        }
    }

    /// Cookies to send to `url` (RFC 6265 §5.4 subset), longest path first.
    pub fn cookies_for(&self, url: &str, now: f64) -> Vec<Cookie> {
        let Ok(u) = Url::parse(url) else {
            return Vec::new();
        };
        let ctx = ReqCtx {
            host: u.host_str().unwrap_or("").to_ascii_lowercase(),
            is_secure: secure_cookie_url(&u),
            path: if u.path().is_empty() { "/" } else { u.path() }.to_string(),
            now,
        };
        let mut out: Vec<Cookie> = self
            .store
            .values()
            .filter(|c| cookie_applies(c, &ctx))
            .cloned()
            .collect();
        // RFC 6265 §5.4: longest path first. Same-path order is unspecified by the
        // spec (and doesn't survive a storageState round-trip via the HashMap store),
        // so tiebreak on name for a stable, deterministic header.
        out.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then_with(|| a.name.cmp(&b.name))
        });
        out
    }

    /// `Cookie:` header value for `url`, or "" if none apply.
    pub fn cookie_header(&self, url: &str, now: f64) -> String {
        self.cookies_for(url, now)
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Apply SameSite at the request boundary in addition to domain/path/security.
    pub fn cookie_header_for_request(
        &self,
        url: &str,
        now: f64,
        context: &crate::request_context::RequestContext,
        method: &str,
    ) -> String {
        self.cookies_for(url, now)
            .into_iter()
            .filter(|cookie| context.allows_same_site_cookie(&cookie.same_site, url, method))
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Script-visible cookie string. HttpOnly cookies remain available to HTTP
    /// requests and storage_state but must never be exposed through document.cookie.
    pub fn document_cookie(&self, url: &str, now: f64) -> String {
        self.cookies_for(url, now)
            .into_iter()
            .filter(|c| !c.http_only)
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Non-HTTP cookie writes cannot create or overwrite an HttpOnly cookie.
    pub fn set_from_document(&mut self, url: &str, line: &str, now: f64) {
        let Ok(u) = Url::parse(url) else {
            return;
        };
        let host = u.host_str().unwrap_or("").to_ascii_lowercase();
        let Some((mut raw, _, _)) = parse_with_raw(line) else {
            return;
        };
        if raw.http_only {
            return;
        }
        let domain = if raw.domain.is_empty() {
            host.clone()
        } else {
            raw.domain.clone()
        };
        raw.path = explicit_cookie_path(line)
            .filter(|path| path.starts_with('/'))
            .unwrap_or_else(|| default_cookie_path(&u).to_owned());
        if self
            .store
            .get(&key(&domain, &raw.path, &raw.name))
            .is_some_and(|c| c.http_only && c.expires_at > now)
        {
            return;
        }
        self.ingest_one(line, &u, now);
    }

    /// Add a cookie record directly (Playwright `addCookies` / storageState seed).
    /// `expires` is epoch SECONDS; `-1`/`None` → session cookie.
    pub fn add(&mut self, name: &str, value: &str, domain: &str, path: &str, expires: Option<f64>) {
        let domain = domain.trim_start_matches('.').to_ascii_lowercase();
        let path = if path.is_empty() { "/" } else { path };
        let c = Cookie {
            name: name.to_string(),
            value: value.to_string(),
            domain: domain.clone(),
            host_only: false,
            path: path.to_string(),
            secure: false,
            http_only: false,
            same_site: "lax".into(),
            expires_at: seed_expiry(expires),
        };
        self.write_cookie(key(&domain, path, name), Some(c));
    }

    /// Every stored cookie (for `storageState` dump).
    pub fn all(&self) -> Vec<Cookie> {
        self.store.values().cloned().collect()
    }

    pub fn size(&self) -> usize {
        self.store.len()
    }

    /// Serialize the jar to a `storageState` JSON string (cross-navigation /
    /// `context.storageState`). Session cookies (Infinity expiry) → `null`.
    pub fn storage_state(&self) -> String {
        let recs: Vec<CookieRecord> = self.store.values().map(CookieRecord::from).collect();
        serde_json::to_string(&recs).unwrap_or_else(|_| "[]".to_string())
    }

    /// Rebuild a jar from a `storage_state` JSON string (best-effort; bad input → empty).
    pub fn from_storage_state(json: &str) -> Self {
        let recs: Vec<CookieRecord> = serde_json::from_str(json).unwrap_or_default();
        let mut jar = CookieJar::new();
        for r in recs {
            let c: Cookie = r.into();
            jar.store.insert(key(&c.domain, &c.path, &c.name), c);
        }
        jar
    }
}

// Serializable record (Infinity expiry is not valid JSON → Option/None = session).
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct CookieRecord {
    #[serde(default)]
    host_only: bool,
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
    http_only: bool,
    same_site: String,
    expires_at: Option<f64>,
}

impl From<&Cookie> for CookieRecord {
    fn from(c: &Cookie) -> Self {
        CookieRecord {
            host_only: c.host_only,
            name: c.name.clone(),
            value: c.value.clone(),
            domain: c.domain.clone(),
            path: c.path.clone(),
            secure: c.secure,
            http_only: c.http_only,
            same_site: c.same_site.clone(),
            expires_at: c.expires_at.is_finite().then_some(c.expires_at),
        }
    }
}

impl From<CookieRecord> for Cookie {
    fn from(r: CookieRecord) -> Self {
        Cookie {
            host_only: r.host_only,
            name: r.name,
            value: r.value,
            domain: r.domain,
            path: r.path,
            secure: r.secure,
            http_only: r.http_only,
            same_site: r.same_site,
            expires_at: r.expires_at.unwrap_or(f64::INFINITY),
        }
    }
}

// Re-parse a Set-Cookie line, returning the cookie plus its raw lifetime
// attributes so ingest can resolve expiry against `now`.
fn parse_with_raw(line: &str) -> Option<(Cookie, Option<f64>, Option<f64>)> {
    let cookie = parse_set_cookie(line)?;
    let (max_age, expires) = raw_lifetime(line);
    Some((cookie, max_age, expires))
}

fn raw_lifetime(line: &str) -> (Option<f64>, Option<f64>) {
    let mut max_age = None;
    let mut expires = None;
    for attr in line.split(';').skip(1) {
        let (k, v) = split_attr(attr);
        match k.as_str() {
            "max-age" => max_age = parse_num(&v),
            "expires" => expires = parse_http_date(&v),
            _ => {}
        }
    }
    (max_age, expires)
}

fn seed_expiry(expires: Option<f64>) -> f64 {
    match expires {
        Some(e) if e >= 0.0 => e * 1000.0,
        _ => f64::INFINITY,
    }
}

// Minimal RFC 1123 / RFC 850 / asctime parser → epoch ms, no chrono dep.
fn httpdate_ms(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let (d, mon, y, time) = date_fields(&parts)?;
    let month = month_num(&mon)?;
    let (hh, mm, ss) = hms(&time)?;
    Some(epoch_ms(y, month, d, hh, mm, ss))
}

// Pull (day, month, year, "hh:mm:ss") out of the supported layouts.
fn date_fields(p: &[&str]) -> Option<(i64, String, i64, String)> {
    match p.len() {
        // "Wdy, DD Mon YYYY HH:MM:SS GMT" (RFC 1123)
        6 => Some((
            p[1].parse().ok()?,
            p[2].to_string(),
            p[3].parse().ok()?,
            p[4].to_string(),
        )),
        // "Wdy Mon D HH:MM:SS YYYY" (asctime)
        5 => Some((
            p[2].parse().ok()?,
            p[1].to_string(),
            p[4].parse().ok()?,
            p[3].to_string(),
        )),
        // "Wdy, DD-Mon-YY HH:MM:SS GMT" (RFC 850)
        4 => rfc850_fields(p),
        _ => None,
    }
}

fn rfc850_fields(p: &[&str]) -> Option<(i64, String, i64, String)> {
    let dmy: Vec<&str> = p[1].split('-').collect();
    if dmy.len() != 3 {
        return None;
    }
    Some((
        dmy[0].parse().ok()?,
        dmy[1].to_string(),
        two_digit_year(dmy[2])?,
        p[2].to_string(),
    ))
}

fn two_digit_year(y: &str) -> Option<i64> {
    let n: i64 = y.parse().ok()?;
    Some(if n < 70 {
        2000 + n
    } else if n < 100 {
        1900 + n
    } else {
        n
    })
}

fn month_num(m: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lc = m.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|x| lc.starts_with(x))
        .map(|i| i as i64 + 1)
}

fn hms(t: &str) -> Option<(i64, i64, i64)> {
    let p: Vec<&str> = t.split(':').collect();
    if p.len() != 3 {
        return None;
    }
    Some((p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?))
}

// Days-from-civil (Howard Hinnant's algorithm) → epoch ms.
fn epoch_ms(y: i64, m: i64, d: i64, hh: i64, mm: i64, ss: i64) -> f64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    ((days * 86400 + hh * 3600 + mm * 60 + ss) * 1000) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_cookie_scope_prefixes_and_default_paths_survive_serialization() {
        let mut jar = CookieJar::new();
        let url = "https://app.example.com/account/login";
        jar.set_from_response(
            url,
            &[
                "host=1".into(),
                "domain=1; Domain=example.com; Path=/".into(),
                "__Host-good=1; Secure; Path=/".into(),
                "__Secure-good=1; Secure; Path=/".into(),
                "__Http-good=1; Secure; HttpOnly; Path=/".into(),
                "__Host-bad=1; Secure".into(),
                "__Host-domain=1; Secure; Path=/; Domain=example.com".into(),
                "__Secure-bad=1".into(),
                "__Http-bad=1; Secure; Path=/".into(),
                "suffix=1; Domain=com; Path=/".into(),
            ],
            0.0,
        );
        assert_eq!(jar.size(), 5);
        assert!(!jar
            .cookie_header("https://child.app.example.com/account/", 0.0)
            .contains("host="));
        assert_eq!(
            jar.cookie_header("https://other.example.com/", 0.0),
            "domain=1"
        );
        assert!(!jar
            .cookie_header("https://app.example.com/", 0.0)
            .contains("host="));
        assert!(jar
            .cookie_header("https://app.example.com/account/", 0.0)
            .contains("host=1"));
        assert!(!jar.document_cookie(url, 0.0).contains("__Http-good="));
        let restored = CookieJar::from_storage_state(&jar.storage_state());
        assert_eq!(
            restored.cookie_header("https://child.app.example.com/account/", 0.0),
            "domain=1"
        );
        jar.set_from_response(
            "http://remote.example.com/",
            &["insecure=1; Secure".into()],
            0.0,
        );
        assert_eq!(jar.size(), 5);
        jar.set_from_document(url, "__Http-script=1; Secure; Path=/", 0.0);
        assert_eq!(jar.size(), 5);
    }

    #[test]
    fn request_cookie_filter_applies_same_site_navigation_and_embedding_rules() {
        use crate::request_context::{Destination, RequestContext, RequestMode};
        let url = "https://target.test/path";
        let mut jar = CookieJar::new();
        jar.set_from_response(
            url,
            &[
                "strict=1; Secure; SameSite=Strict".into(),
                "lax=1; Secure; SameSite=Lax".into(),
                "default=1; Secure".into(),
                "none=1; Secure; SameSite=None".into(),
            ],
            0.0,
        );
        let names = |context: &RequestContext, method| {
            let header = jar.cookie_header_for_request(url, 0.0, context, method);
            let mut names: Vec<_> = header
                .split("; ")
                .filter_map(|cookie| cookie.split_once('=').map(|pair| pair.0.to_owned()))
                .collect();
            names.sort_unstable();
            names
        };
        let same = RequestContext::resource(
            "https://target.test/",
            Destination::Empty,
            RequestMode::Cors,
        );
        assert_eq!(names(&same, "GET"), ["default", "lax", "none", "strict"]);
        let cross =
            RequestContext::resource("https://other.test/", Destination::Empty, RequestMode::Cors);
        assert_eq!(names(&cross, "GET"), ["none"]);
        let navigation = RequestContext::navigation(Some("https://other.test/".into()), true);
        assert_eq!(names(&navigation, "GET"), ["default", "lax", "none"]);
        assert_eq!(names(&navigation, "POST"), ["none"]);
        let mut iframe = navigation.clone();
        iframe.destination = Destination::Iframe;
        assert_eq!(names(&iframe, "GET"), ["none"]);
        let mut embedded = same.clone();
        embedded.top_level_source = Some("https://other.test/".into());
        assert_eq!(names(&embedded, "GET"), ["none"]);
        embedded.top_level_source = Some("https://target.test/".into());
        embedded.cookie_site_is_opaque = true;
        assert_eq!(names(&embedded, "GET"), ["none"]);
        let mut tainted = same;
        tainted.follow_redirect(url, "https://other.test/");
        tainted.follow_redirect("https://other.test/", url);
        assert_eq!(names(&tainted, "GET"), ["none"]);
    }

    #[test]
    fn concurrent_request_merges_preserve_unrelated_network_and_script_cookies() {
        let mut shared = CookieJar::new();
        shared.add("initial", "1", "x.test", "/", None);
        let mut first = shared.fork_for_request();
        let mut second = shared.fork_for_request();
        shared.set_from_document("https://x.test/", "script=1", 0.0);
        first.set_from_response("https://x.test/", &["first=1; HttpOnly".into()], 0.0);
        second.set_from_response("https://x.test/", &["second=2".into()], 0.0);
        shared.merge_request_changes(first);
        shared.merge_request_changes(second);
        assert_eq!(
            shared.cookie_header("https://x.test/", 0.0),
            "first=1; initial=1; script=1; second=2"
        );
        assert!(!shared
            .document_cookie("https://x.test/", 0.0)
            .contains("first="));
        assert!(shared.request_changes.is_none());
    }

    #[test]
    fn request_merge_tracks_same_value_responses_and_deletion_of_absent_snapshot_keys() {
        let mut shared = CookieJar::new();
        shared.add("same", "original", "x.test", "/", None);
        shared.add("scoped", "root", "x.test", "/", None);
        shared.add("scoped", "app", "x.test", "/app", None);
        let mut response = shared.fork_for_request();
        shared.set_from_document("https://x.test/", "same=script", 0.0);
        shared.set_from_document("https://x.test/", "later=script", 0.0);
        response.set_from_response(
            "https://x.test/",
            &[
                "same=original".into(),
                "later=gone; Max-Age=0".into(),
                "scoped=gone; Path=/app; Max-Age=0".into(),
                "rejected=1; Domain=other.test".into(),
            ],
            0.0,
        );
        shared.merge_request_changes(response);
        assert_eq!(
            shared.cookie_header("https://x.test/app", 0.0),
            "same=original; scoped=root"
        );
    }

    #[test]
    fn ingest_and_send_scoped() {
        let mut j = CookieJar::new();
        j.set_from_response(
            "https://x.test/app",
            &["sid=abc; Path=/; HttpOnly".to_string()],
            0.0,
        );
        assert_eq!(j.cookie_header("https://x.test/app/page", 0.0), "sid=abc");
        assert_eq!(j.cookie_header("https://other.test/", 0.0), "");
    }

    #[test]
    fn secure_only_over_https() {
        let mut j = CookieJar::new();
        j.set_from_response("https://x.test/", &["s=1; Secure".to_string()], 0.0);
        assert_eq!(j.cookie_header("http://x.test/", 0.0), "");
        assert_eq!(j.cookie_header("https://x.test/", 0.0), "s=1");
    }

    #[test]
    fn max_age_zero_deletes() {
        let mut j = CookieJar::new();
        j.set_from_response("https://x.test/", &["a=1".to_string()], 0.0);
        j.set_from_response("https://x.test/", &["a=1; Max-Age=0".to_string()], 0.0);
        assert_eq!(j.size(), 0);
    }

    #[test]
    fn longest_path_first() {
        let mut j = CookieJar::new();
        j.set_from_response("https://x.test/", &["a=1; Path=/".to_string()], 0.0);
        j.set_from_response("https://x.test/app", &["b=2; Path=/app".to_string()], 0.0);
        assert_eq!(j.cookie_header("https://x.test/app/x", 0.0), "b=2; a=1");
    }

    #[test]
    fn samesite_none_requires_secure() {
        let mut j = CookieJar::new();
        j.set_from_response("https://x.test/", &["a=1; SameSite=None".to_string()], 0.0);
        assert_eq!(j.size(), 0);
    }

    #[test]
    fn http_date_expiry() {
        let ms = httpdate_ms("Wed, 21 Oct 2015 07:28:00 GMT").unwrap();
        assert_eq!(ms, 1445412480000.0);
    }

    #[test]
    fn add_seed_session_and_dump() {
        let mut j = CookieJar::new();
        j.add("k", "v", "x.test", "/", None);
        assert_eq!(j.cookie_header("https://x.test/", 0.0), "k=v");
        assert_eq!(j.all().len(), 1);
    }

    #[test]
    fn httponly_samesite_and_empty_path_attrs() {
        let mut j = CookieJar::new();
        j.set_from_response(
            "https://x.test/",
            &["a=1; HttpOnly; SameSite=Lax; Path=".to_string()],
            0.0,
        );
        let c = &j.cookies_for("https://x.test/", 0.0)[0];
        assert!(c.http_only);
        assert_eq!(c.same_site, "lax");
        assert_eq!(c.path, "/"); // empty Path attr defaults to "/"
    }

    #[test]
    fn explicit_domain_attr_and_reject_mismatch() {
        let mut j = CookieJar::new();
        // Domain the response host is within → kept.
        j.set_from_response(
            "https://app.x.test/",
            &["a=1; Domain=x.test".to_string()],
            0.0,
        );
        assert_eq!(j.cookie_header("https://x.test/", 0.0), "a=1");
        // Domain the response host is NOT within → rejected.
        j.set_from_response(
            "https://x.test/",
            &["b=2; Domain=other.test".to_string()],
            0.0,
        );
        assert!(!j.cookie_header("https://other.test/", 0.0).contains("b=2"));
    }

    #[test]
    fn path_match_prefix_boundary() {
        let mut j = CookieJar::new();
        j.set_from_response("https://x.test/app", &["a=1; Path=/app".to_string()], 0.0);
        // "/app" matches "/app/x" (next char is '/') but not "/application".
        assert_eq!(j.cookie_header("https://x.test/app/x", 0.0), "a=1");
        assert_eq!(j.cookie_header("https://x.test/application", 0.0), "");
    }

    #[test]
    fn expires_header_kept_and_add_with_seconds() {
        let mut j = CookieJar::new();
        // Expires in the past relative to a large `now` → deleted.
        j.set_from_response(
            "https://x.test/",
            &["a=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT".to_string()],
            2_000_000_000_000.0,
        );
        assert_eq!(j.size(), 0);
        // add() with epoch-seconds expiry in the far future → kept.
        j.add("k", "v", "x.test", "/p", Some(4_000_000_000.0));
        assert_eq!(j.cookie_header("https://x.test/p", 0.0), "k=v");
    }

    #[test]
    fn http_date_formats() {
        // asctime (5 fields)
        assert!(httpdate_ms("Sun Nov 6 08:49:37 1994").is_some());
        // rfc850 with 2-digit year
        assert!(httpdate_ms("Sunday, 06-Nov-94 08:49:37 GMT").is_some());
        // rfc850 with 4-digit year (two_digit_year >= 100 branch)
        assert!(httpdate_ms("Sunday, 06-Nov-1994 08:49:37 GMT").is_some());
        // bad month / bad time / wrong field count → None
        assert!(httpdate_ms("Wed, 21 Xyz 2015 07:28:00 GMT").is_none());
        assert!(httpdate_ms("Wed, 21 Oct 2015 0728 GMT").is_none());
        assert!(httpdate_ms("garbage").is_none());
    }

    #[test]
    fn two_digit_year_pivots() {
        assert_eq!(two_digit_year("69"), Some(2069));
        assert_eq!(two_digit_year("70"), Some(1970));
        assert_eq!(two_digit_year("2024"), Some(2024));
    }

    #[test]
    fn malformed_url_inputs_are_safe() {
        let mut j = CookieJar::new();
        j.set_from_response("not a url", &["a=1".to_string()], 0.0); // no panic, no store
        assert_eq!(j.size(), 0);
        assert!(j.cookies_for("not a url", 0.0).is_empty());
    }

    #[test]
    fn unknown_attr_and_bad_namevalue_dropped() {
        let mut j = CookieJar::new();
        // unknown attribute is ignored; cookie still stored
        j.set_from_response("https://x.test/", &["a=1; Weird=zzz".to_string()], 0.0);
        assert_eq!(j.cookie_header("https://x.test/", 0.0), "a=1");
        // empty name and no-"=" lines are rejected (parse None → ingest skip)
        j.set_from_response("https://x.test/", &["=v".to_string()], 0.0);
        j.set_from_response("https://x.test/", &["novalueatall".to_string()], 0.0);
        assert_eq!(j.size(), 1);
    }

    #[test]
    fn path_not_a_prefix_excludes_cookie() {
        let mut j = CookieJar::new();
        j.set_from_response("https://x.test/foo", &["a=1; Path=/foo".to_string()], 0.0);
        // request path doesn't start with the cookie path → excluded (outer false)
        assert_eq!(j.cookie_header("https://x.test/bar", 0.0), "");
    }

    #[test]
    fn storage_state_round_trip() {
        let mut j = CookieJar::new();
        j.set_from_response(
            "https://x.test/app",
            &["sid=abc; Path=/; Secure".to_string()],
            0.0,
        );
        j.add("k", "v", "x.test", "/", None); // session cookie (Infinity)
        let state = j.storage_state();
        let j2 = CookieJar::from_storage_state(&state);
        assert_eq!(j2.size(), 2);
        // secure cookie still secure-gated; session cookie still sent
        // same path "/" → deterministic name order (k before sid)
        assert_eq!(j2.cookie_header("https://x.test/app", 0.0), "k=v; sid=abc");
        assert_eq!(j2.cookie_header("http://x.test/", 0.0), "k=v"); // sid is Secure
    }

    #[test]
    fn from_bad_storage_state_is_empty() {
        assert_eq!(CookieJar::from_storage_state("not json").size(), 0);
    }

    #[test]
    fn rfc850_with_bad_day_month_year_field() {
        // 4 tokens but the date field doesn't split into 3 on '-' → None.
        assert!(httpdate_ms("Mon, 06Nov94 08:49:37 GMT").is_none());
    }
}
