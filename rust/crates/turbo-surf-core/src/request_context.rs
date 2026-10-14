//! Browser request metadata and origin-scoped client-hint negotiation.
//!
//! A hint store belongs to a browsing session. Clones share the store so page
//! fetches and navigations can use the same opt-in without leaking it to another
//! session. This does not implement Permissions-Policy delegation or Critical-CH
//! retries; cross-origin subresources therefore receive no high-entropy hints.

use crate::fingerprint::{Profile, ACCEPT};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Destination {
    #[default]
    Document,
    Iframe,
    Script,
    Style,
    Image,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestMode {
    #[default]
    Navigate,
    Cors,
    NoCors,
    SameOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Credentials {
    #[default]
    Include,
    SameOrigin,
    Omit,
}

#[derive(Debug, Clone)]
pub struct RequestContext {
    pub destination: Destination,
    pub mode: RequestMode,
    pub source: Option<String>,
    /// A stylesheet resource can supply a referrer distinct from its document origin.
    pub referrer: Option<String>,
    pub referrer_policy: crate::referrer_policy::ReferrerPolicy,
    pub top_level_source: Option<String>,
    /// Opaque or cross-site embedding ancestors make the cookie site opaque.
    pub cookie_site_is_opaque: bool,
    pub user_activation: bool,
    pub credentials: Credentials,
    /// COEP credentialless omits credentials for cross-origin no-cors hops.
    pub credentialless: bool,
    /// Sandboxed/data documents can have an opaque origin despite a tuple URL.
    pub opaque_origin: bool,
    /// Redirect-tainted request Origin is distinct from the document's cookie site.
    pub origin_tainted: bool,
    pub redirect_site: Option<String>,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::navigation(None, true)
    }
}

impl RequestContext {
    pub fn navigation(source: Option<String>, user_activation: bool) -> Self {
        Self {
            destination: Destination::Document,
            mode: RequestMode::Navigate,
            source,
            referrer: None,
            referrer_policy: Default::default(),
            top_level_source: None,
            cookie_site_is_opaque: false,
            user_activation,
            credentials: Credentials::Include,
            credentialless: false,
            opaque_origin: false,
            origin_tainted: false,
            redirect_site: None,
        }
    }

    pub fn resource(source: &str, destination: Destination, mode: RequestMode) -> Self {
        Self {
            destination,
            mode,
            source: Some(source.to_string()),
            referrer: None,
            referrer_policy: Default::default(),
            top_level_source: None,
            cookie_site_is_opaque: false,
            user_activation: false,
            credentials: if mode == RequestMode::Cors {
                Credentials::SameOrigin
            } else {
                Credentials::Include
            },
            credentialless: false,
            opaque_origin: false,
            origin_tainted: false,
            redirect_site: None,
        }
    }

    pub fn sends_credentials(&self, target: &str) -> bool {
        if self.credentialless
            && self.mode == RequestMode::NoCors
            && (self.opaque_origin
                || self
                    .redirect_site
                    .as_deref()
                    .is_some_and(|site| site != "same-origin")
                || !self
                    .source
                    .as_deref()
                    .is_some_and(|source| same_origin(source, target)))
        {
            return false;
        }
        match self.credentials {
            Credentials::Include => true,
            Credentials::SameOrigin => self.source.as_deref().is_some_and(|source| {
                !self.opaque_origin && !self.origin_tainted && same_origin(source, target)
            }),
            Credentials::Omit => false,
        }
    }

    pub fn allows_same_site_cookie(
        &self,
        same_site_value: &str,
        target: &str,
        method: &str,
    ) -> bool {
        if same_site_value.eq_ignore_ascii_case("none") {
            return true;
        }
        let same_site_request = !self.opaque_origin
            && self.redirect_site.as_deref() != Some("cross-site")
            && !self.cookie_site_is_opaque
            && self.source.as_deref().map_or(
                self.mode == RequestMode::Navigate && self.destination == Destination::Document,
                |source| same_site(source, target),
            )
            && self
                .top_level_source
                .as_deref()
                .is_none_or(|source| same_site(source, target));
        same_site_request
            || !same_site_value.eq_ignore_ascii_case("strict")
                && self.mode == RequestMode::Navigate
                && self.destination == Destination::Document
                && matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE")
    }

    pub fn apply_referrer_policy_headers(&mut self, headers: &[(String, String)]) {
        let value = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("referrer-policy"))
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>()
            .join(",");
        if let Some(policy) = crate::referrer_policy::ReferrerPolicy::from_header(&value) {
            self.referrer_policy = policy;
        }
    }

    /// Carry Fetch's origin taint and largest site relationship through a chain.
    pub fn follow_redirect(&mut self, current: &str, next: &str) {
        let relation = |target: &str| match self.source.as_deref() {
            None => "none",
            Some(_) if self.opaque_origin => "cross-site",
            Some(source) if same_origin(source, target) => "same-origin",
            Some(source) if same_site(source, target) => "same-site",
            _ => "cross-site",
        };
        let previous = self
            .redirect_site
            .as_deref()
            .unwrap_or_else(|| relation(current));
        let next_site = relation(next);
        let rank = |site: &str| match site {
            "cross-site" => 2,
            "same-site" => 1,
            _ => 0,
        };
        let site = if rank(previous) > rank(next_site) {
            previous
        } else {
            next_site
        }
        .to_owned();
        if !same_origin(current, next)
            && self
                .source
                .as_deref()
                .is_some_and(|source| !same_origin(source, current))
        {
            self.origin_tainted = true;
        }
        self.redirect_site = Some(site);
    }

    pub fn headers(&self, target: &str, method: &str) -> BTreeMap<String, String> {
        let mut h = BTreeMap::new();
        let accept = match self.destination {
            Destination::Document | Destination::Iframe => ACCEPT,
            Destination::Style => "text/css,*/*;q=0.1",
            Destination::Image => {
                "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8"
            }
            _ => "*/*",
        };
        h.insert("accept".into(), accept.into());
        // Incremental delivery helps document/fetch/image streams; blocking
        // scripts and styles prefer a complete response at their urgency.
        let priority = match self.destination {
            Destination::Document | Destination::Iframe => "u=0, i",
            Destination::Style => "u=0",
            Destination::Script => "u=1",
            Destination::Image => "u=2, i",
            Destination::Empty => "u=1, i",
        };
        h.insert("priority".into(), priority.into());
        if trustworthy(target) {
            let site = match self.source.as_deref() {
                None => "none",
                Some(_) if self.opaque_origin => "cross-site",
                Some(source) if same_origin(source, target) => "same-origin",
                Some(source) if same_site(source, target) => "same-site",
                _ => "cross-site",
            };
            h.insert(
                "sec-fetch-site".into(),
                self.redirect_site.as_deref().unwrap_or(site).into(),
            );
            h.insert(
                "sec-fetch-mode".into(),
                match self.mode {
                    RequestMode::Navigate => "navigate",
                    RequestMode::Cors => "cors",
                    RequestMode::NoCors => "no-cors",
                    RequestMode::SameOrigin => "same-origin",
                }
                .into(),
            );
            h.insert(
                "sec-fetch-dest".into(),
                match self.destination {
                    Destination::Document => "document",
                    Destination::Iframe => "iframe",
                    Destination::Script => "script",
                    Destination::Style => "style",
                    Destination::Image => "image",
                    Destination::Empty => "empty",
                }
                .into(),
            );
            if self.mode == RequestMode::Navigate && self.user_activation {
                h.insert("sec-fetch-user".into(), "?1".into());
            }
        }
        if self.mode == RequestMode::Navigate {
            h.insert("upgrade-insecure-requests".into(), "1".into());
        }
        if let Some(source) = self.source.as_deref() {
            if let Some(referer) = self
                .referrer_policy
                .referrer(self.referrer.as_deref().unwrap_or(source), target)
            {
                h.insert("referer".into(), referer);
            }
            // CORS script requests carry Origin even on the same URL origin;
            // ordinary fetch adds it cross-origin or for unsafe methods.
            if (self.mode == RequestMode::Cors
                && (self.destination == Destination::Script
                    || self.opaque_origin
                    || self.origin_tainted
                    || !same_origin(source, target)))
                || !matches!(method, "GET" | "HEAD")
            {
                if self.opaque_origin || self.origin_tainted {
                    h.insert("origin".into(), "null".into());
                } else if let Some(origin) = origin(source) {
                    h.insert("origin".into(), origin);
                }
            }
        }
        h
    }
}

pub fn origin(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.origin().ascii_serialization())
}

pub fn same_origin(source: &str, target: &str) -> bool {
    origin(source).is_some_and(|source| origin(target).as_deref() == Some(&source))
}

pub fn same_site(source: &str, target: &str) -> bool {
    let (Ok(source), Ok(target)) = (Url::parse(source), Url::parse(target)) else {
        return false;
    };
    let (Some(source_host), Some(target_host)) = (source.host_str(), target.host_str()) else {
        return false;
    };
    source.scheme() == target.scheme()
        && (source_host == target_host
            || psl::domain_str(source_host)
                .zip(psl::domain_str(target_host))
                .is_some_and(|(a, b)| a == b))
}

pub fn trustworthy(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    url.scheme() == "https"
        || (url.scheme() == "http"
            && url.host_str().is_some_and(|host| {
                host == "localhost"
                    || host.ends_with(".localhost")
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            }))
}

#[derive(Clone, Default)]
pub struct ClientHints(Arc<HintState>);

impl std::fmt::Debug for ClientHints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientHints").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct HintState {
    origins: Mutex<BTreeMap<String, BTreeSet<String>>>,
    // The browsing session's transport pool must survive navigation and page
    // runtime replacement, just like its negotiated hints.
    client: OnceLock<crate::http_backend::Client>,
}

impl ClientHints {
    pub(crate) fn manual_client(&self) -> crate::http_backend::Client {
        self.0
            .client
            .get_or_init(crate::net::build_browser_client)
            .clone()
    }
    pub fn observe(&self, url: &str, accept_ch: Option<&str>, clear: bool) {
        if !trustworthy(url) {
            return;
        }
        let Some(origin) = origin(url) else { return };
        let mut origins = self.0.origins.lock().unwrap_or_else(|e| e.into_inner());
        if clear {
            origins.remove(&origin);
        }
        if let Some(value) = accept_ch {
            origins.insert(
                origin,
                value
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .collect(),
            );
        }
    }

    pub fn headers(
        &self,
        target: &str,
        context: &RequestContext,
        profile: &Profile,
    ) -> BTreeMap<String, String> {
        let mut headers = BTreeMap::new();
        if !trustworthy(target) {
            return headers;
        }
        headers.insert("sec-ch-ua".into(), profile.sec_ch_ua.clone());
        headers.insert("sec-ch-ua-mobile".into(), "?0".into());
        headers.insert(
            "sec-ch-ua-platform".into(),
            profile.sec_ch_ua_platform.into(),
        );
        if context.opaque_origin
            || context.destination != Destination::Document
                && context
                    .source
                    .as_deref()
                    .is_some_and(|source| !same_origin(source, target))
        {
            return headers;
        }
        let Some(origin) = origin(target) else {
            return headers;
        };
        let origins = self.0.origins.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(requested) = origins.get(&origin) {
            for name in requested {
                if let Some(value) = hint_value(name, profile) {
                    headers.insert(name.clone(), value);
                }
            }
        }
        headers
    }
}

fn hint_value(name: &str, profile: &Profile) -> Option<String> {
    let value = match name {
        "sec-ch-ua-arch" => format!("\"{}\"", profile.architecture()),
        "sec-ch-ua-bitness" => "\"64\"".into(),
        "sec-ch-ua-model" => "\"\"".into(),
        "sec-ch-ua-wow64" => "?0".into(),
        "sec-ch-ua-form-factors" => "\"Desktop\"".into(),
        "sec-ch-ua-platform-version" => format!("\"{}\"", profile.platform_version()),
        "sec-ch-ua-full-version" => format!("\"{}\"", profile.full_version),
        "sec-ch-ua-full-version-list" => {
            crate::fingerprint::brand_header(profile.chrome_major, profile.full_version, true)
        }
        "device-memory" | "sec-ch-device-memory" => profile.device_memory.to_string(),
        // Match the render tier's estimated NetworkInformation defaults.
        "downlink" => "10".into(),
        "rtt" => "50".into(),
        "sec-ch-prefers-color-scheme" => "light".into(),
        "sec-ch-prefers-reduced-motion" => "no-preference".into(),
        _ => return None,
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_origin_taint_preserves_cookie_site_and_fetch_site_chain() {
        let mut context = RequestContext::resource(
            "https://source.test/page",
            Destination::Script,
            RequestMode::Cors,
        );
        context.credentials = Credentials::Include;
        context.follow_redirect(
            "https://source.test:8443/module",
            "https://source.test/final",
        );
        assert!(context.origin_tainted);
        assert!(!context.opaque_origin);
        let headers = context.headers("https://source.test/final", "GET");
        assert_eq!(headers["origin"], "null");
        assert_eq!(headers["sec-fetch-site"], "same-site");
        assert!(context.allows_same_site_cookie("lax", "https://source.test/final", "GET"));
        context.credentials = Credentials::SameOrigin;
        assert!(!context.sends_credentials("https://source.test/final"));
        context.follow_redirect("https://source.test/final", "https://other.test/module");
        context.follow_redirect("https://other.test/module", "https://source.test/back");
        assert_eq!(
            context.headers("https://source.test/back", "GET")["sec-fetch-site"],
            "cross-site"
        );
        assert!(!context.allows_same_site_cookie("strict", "https://source.test/back", "GET"));
    }

    #[test]
    fn opaque_actors_do_not_inherit_url_origin_credentials_or_client_hints() {
        let source = "https://example.com/sandbox";
        let mut context = RequestContext::resource(source, Destination::Empty, RequestMode::Cors);
        context.opaque_origin = true;
        assert!(!context.sends_credentials(source));
        let headers = context.headers(source, "GET");
        assert_eq!(headers["origin"], "null");
        assert_eq!(headers["sec-fetch-site"], "cross-site");
        assert_eq!(headers["referer"], source);
        let hints = ClientHints::default();
        hints.observe(source, Some("sec-ch-ua-full-version"), false);
        assert!(!hints
            .headers(source, &context, &crate::fingerprint::default_profile())
            .contains_key("sec-ch-ua-full-version"));
    }

    #[test]
    fn resource_referrer_does_not_change_the_document_origin_or_credentials() {
        let mut context = RequestContext::resource(
            "https://document.test/page",
            Destination::Style,
            RequestMode::Cors,
        );
        context.referrer = Some("https://styles.test/assets/main.css".into());
        let headers = context.headers("https://styles.test/assets/import.css", "GET");
        assert_eq!(
            headers.get("referer").map(String::as_str),
            Some("https://styles.test/assets/main.css")
        );
        assert_eq!(
            headers.get("origin").map(String::as_str),
            Some("https://document.test")
        );
        assert_eq!(
            headers.get("sec-fetch-site").map(String::as_str),
            Some("cross-site")
        );
        assert!(!context.sends_credentials("https://styles.test/assets/import.css"));
    }

    #[test]
    fn contexts_distinguish_navigation_script_and_cors() {
        let nav = RequestContext::navigation(Some("https://example.com/?q=1#hash".into()), false);
        let h = nav.headers("https://example.com/search", "GET");
        assert_eq!(h["sec-fetch-mode"], "navigate");
        assert_eq!(h["priority"], "u=0, i");
        assert_eq!(h["referer"], "https://example.com/?q=1");
        assert!(!h.contains_key("origin"));
        assert!(!h.contains_key("sec-fetch-user"));
        let script = RequestContext::resource(
            "https://example.com/p",
            Destination::Script,
            RequestMode::NoCors,
        );
        let h = script.headers("https://cdn.example.net/s.js", "GET");
        assert_eq!(h["sec-fetch-dest"], "script");
        assert_eq!(h["sec-fetch-mode"], "no-cors");
        assert_eq!(h["priority"], "u=1");
        assert_eq!(h["referer"], "https://example.com/");
        assert!(!h.contains_key("origin"));
        assert!(!h.contains_key("upgrade-insecure-requests"));
        let cors = RequestContext::resource(
            "https://example.com/p",
            Destination::Empty,
            RequestMode::Cors,
        );
        assert_eq!(
            cors.headers("https://api.test/data", "GET")["origin"],
            "https://example.com"
        );
        assert!(!cors
            .headers("https://example.com/data", "GET")
            .contains_key("origin"));
        assert_eq!(
            cors.headers("https://example.com/data", "POST")["origin"],
            "https://example.com"
        );
        assert!(!cors
            .headers("http://api.test/data", "GET")
            .contains_key("referer"));
    }

    #[test]
    fn schemeful_site_uses_public_suffix_wildcards_and_exceptions() {
        let ctx = RequestContext::resource(
            "https://a.example.co.uk",
            Destination::Script,
            RequestMode::NoCors,
        );
        assert_eq!(
            ctx.headers("https://b.example.co.uk", "GET")["sec-fetch-site"],
            "same-site"
        );
        assert_eq!(
            ctx.headers("https://other.co.uk", "GET")["sec-fetch-site"],
            "cross-site"
        );
        assert_eq!(
            ctx.headers("https://example.co.uk", "GET")["sec-fetch-site"],
            "same-site"
        );
        let private =
            RequestContext::resource("https://a.github.io", Destination::Empty, RequestMode::Cors);
        assert_eq!(
            private.headers("https://b.github.io", "GET")["sec-fetch-site"],
            "cross-site"
        );
        let port = RequestContext::resource(
            "https://example.com:444",
            Destination::Empty,
            RequestMode::Cors,
        );
        assert_eq!(
            port.headers("https://example.com:445", "GET")["sec-fetch-site"],
            "same-site"
        );
        assert!(!same_site("http://example.com", "https://example.com"));
    }

    #[test]
    fn hints_are_opt_in_origin_scoped_and_session_scoped() {
        let hints = ClientHints::default();
        let profile = crate::fingerprint::default_profile();
        let context = RequestContext::default();
        let url = "https://example.com/a";
        assert_eq!(hints.headers(url, &context, &profile).len(), 3);
        hints.observe(
            url,
            Some("Sec-CH-UA-Arch, Sec-CH-UA-Full-Version-List, unknown"),
            false,
        );
        let h = hints.clone().headers(url, &context, &profile);
        assert_eq!(h["sec-ch-ua-arch"], "\"arm\"");
        assert!(h["sec-ch-ua-full-version-list"].contains(profile.full_version));
        assert!(!h.contains_key("unknown"));
        assert_eq!(
            hints
                .headers("https://example.com:444/a", &context, &profile)
                .len(),
            3
        );
        assert_eq!(
            ClientHints::default()
                .headers(url, &context, &profile)
                .len(),
            3
        );
        let subresource = RequestContext::resource(
            "https://other.com",
            Destination::Script,
            RequestMode::NoCors,
        );
        assert_eq!(hints.headers(url, &subresource, &profile).len(), 3);
        hints.observe(url, Some(""), false);
        assert_eq!(hints.headers(url, &context, &profile).len(), 3);
        hints.observe(url, Some("sec-ch-ua-arch"), false);
        hints.observe(url, None, true);
        assert_eq!(hints.headers(url, &context, &profile).len(), 3);
        hints.observe("http://example.com", Some("sec-ch-ua-arch"), false);
        assert!(hints
            .headers("http://example.com", &context, &profile)
            .is_empty());
    }
}

#[cfg(test)]
mod credentialless_tests {
    use super::*;

    #[test]
    fn credentialless_cookie_admission_tracks_the_entire_redirect_chain() {
        let source = "https://source.test";
        let mut context =
            RequestContext::resource(source, Destination::Script, RequestMode::NoCors);
        context.credentialless = true;
        assert!(context.sends_credentials(source));
        assert!(!context.sends_credentials("https://cross.test"));
        context.follow_redirect(source, "https://cross.test");
        assert!(!context.sends_credentials("https://cross.test"));
        context.follow_redirect("https://cross.test", source);
        assert!(!context.sends_credentials(source));
        context.mode = RequestMode::Cors;
        assert!(context.sends_credentials(source));
        context.mode = RequestMode::NoCors;
        context.opaque_origin = true;
        assert!(!context.sends_credentials(source));
    }
}
