//! URL helpers (port of `src/url.mjs`): resolve, canonicalize, is_http_url.
//! Canonicalization (SPEC §9) drives frontier dedupe.

use url::Url;

/// Resolve a possibly-relative `href` against `base`. Returns an absolute URL
/// string, or `None` if it cannot be resolved. Absolute hrefs ignore the base
/// (matching `new URL(href, base)`).
pub fn resolve(base: &str, href: &str) -> Option<String> {
    if href.is_empty() {
        return None;
    }
    if let Ok(u) = Url::parse(href) {
        return Some(u.to_string());
    }
    let b = Url::parse(base).ok()?;
    b.join(href).ok().map(|u| u.to_string())
}

// Tracking/query params stripped during canonicalization (dedupe noise).
const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "gclid",
    "fbclid",
    "mc_cid",
    "mc_eid",
    "ref",
    "ref_src",
];

fn is_tracking(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    TRACKING_PARAMS.contains(&k.as_str())
}

/// Canonical form of a URL for dedupe (SPEC §9): lowercase host (the `url` crate
/// does this on parse), drop fragment, strip known tracking params, sort the
/// remaining query.
pub fn canonicalize(url: &str) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    u.set_fragment(None);

    let mut kept: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| !is_tracking(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    kept.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // The path is left as parsed: the `url` crate already gives special schemes
    // (http/https) "/", and an opaque path cannot be set (as in the JS canonicalizer).
    apply_query(&mut u, &kept);
    Some(u.to_string())
}

fn apply_query(u: &mut Url, kept: &[(String, String)]) {
    if kept.is_empty() {
        u.set_query(None);
        return;
    }
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in kept {
        ser.append_pair(k, v);
    }
    u.set_query(Some(&ser.finish()));
}

/// True if `url` is an http(s) URL we are willing to navigate to.
pub fn is_http_url(url: &str) -> bool {
    match Url::parse(url) {
        Ok(u) => matches!(u.scheme(), "http" | "https"),
        Err(_) => false,
    }
}

/// Host (`hostname:port` minus default port) of a URL, or `None`.
pub fn host_of(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    u.host_str().map(|h| match u.port() {
        Some(p) => format!("{h}:{p}"),
        None => h.to_string(),
    })
}

/// Origin (`scheme://host[:port]`) of a URL, or `None`.
pub fn origin_of(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    Some(u.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_relative_and_absolute() {
        assert_eq!(
            resolve("https://x.test/a/b", "../c").as_deref(),
            Some("https://x.test/c")
        );
        assert_eq!(
            resolve("https://x.test/", "https://y.test/z").as_deref(),
            Some("https://y.test/z")
        );
        assert_eq!(resolve("https://x.test/", ""), None);
    }

    #[test]
    fn canonicalize_strips_tracking_and_sorts() {
        let c = canonicalize("https://X.test/p?b=2&utm_source=g&a=1#frag").unwrap();
        assert_eq!(c, "https://x.test/p?a=1&b=2");
    }

    #[test]
    fn canonicalize_adds_root_path() {
        assert_eq!(canonicalize("https://x.test").unwrap(), "https://x.test/");
    }

    #[test]
    fn http_url_gate() {
        assert!(is_http_url("http://x.test"));
        assert!(is_http_url("https://x.test"));
        assert!(!is_http_url("mailto:a@b.test"));
        assert!(!is_http_url("not a url"));
    }

    #[test]
    fn host_of_includes_nondefault_port() {
        assert_eq!(host_of("https://x.test/a").as_deref(), Some("x.test"));
        assert_eq!(
            host_of("https://x.test:8443/a").as_deref(),
            Some("x.test:8443")
        );
        assert_eq!(host_of("mailto:a@b.test"), None);
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn origin_of_serializes_scheme_host_port() {
        assert_eq!(
            origin_of("https://x.test/a?b=1").as_deref(),
            Some("https://x.test")
        );
        assert_eq!(
            origin_of("http://x.test:81/a").as_deref(),
            Some("http://x.test:81")
        );
        assert_eq!(origin_of("not a url"), None);
    }

    #[test]
    fn canonicalize_leaves_an_empty_opaque_path_alone() {
        // Like the JS canonicalizer (an opaque path's pathname cannot be set), only the
        // fragment and query change: `new URL("foo:?b=2&a=1")` keeps `foo:` (turbo-surf#57).
        assert_eq!(
            canonicalize("foo:?b=2&a=1#frag").as_deref(),
            Some("foo:?a=1&b=2")
        );
    }
}
