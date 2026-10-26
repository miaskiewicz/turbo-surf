//! Browser request metadata and origin-scoped client-hint negotiation.
//!
//! A hint store belongs to a browsing session. Clones share the store so page
//! fetches and navigations can use the same opt-in without leaking it to another
//! session. This does not implement Permissions-Policy delegation or Critical-CH
//! retries; cross-origin subresources therefore receive no high-entropy hints.

use crate::fingerprint::Profile;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
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
    Report,
}

impl Destination {
    /// The `Sec-Fetch-Dest` token, also the key of the registry's per-destination values.
    pub fn as_str(self) -> &'static str {
        match self {
            Destination::Document => "document",
            Destination::Iframe => "iframe",
            Destination::Script => "script",
            Destination::Style => "style",
            Destination::Image => "image",
            Destination::Empty => "empty",
            Destination::Report => "report",
        }
    }
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
        // Values the default release measured per destination win; the tables
        // below are the long-standing defaults for releases measured before the
        // identity battery recorded subresources.
        let release = crate::browser::default_release();
        let destination = self.destination.as_str();
        let accept = match self.destination {
            Destination::Document | Destination::Iframe => release.navigation_accept,
            _ => release
                .accept_for(destination)
                .unwrap_or(match self.destination {
                    Destination::Style => "text/css,*/*;q=0.1",
                    Destination::Image => {
                        "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8"
                    }
                    _ => "*/*",
                }),
        };
        h.insert("accept".into(), accept.into());
        // Incremental delivery helps document/fetch/image streams; blocking
        // scripts and styles prefer a complete response at their urgency.
        let priority = release
            .priority_for(destination)
            .unwrap_or(match self.destination {
                Destination::Document | Destination::Iframe => "u=0, i",
                Destination::Style => "u=0",
                Destination::Script => "u=1",
                Destination::Image => "u=2, i",
                Destination::Empty | Destination::Report => "u=1, i",
            });
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
            h.insert("sec-fetch-dest".into(), destination.into());
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

/// The schemeful site used to partition browsing-session state. Ports and
/// subdomains do not distinguish a site; private PSL suffixes still do.
pub fn schemeful_site(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = match url.host()? {
        url::Host::Domain(domain) => psl::domain_str(domain).unwrap_or(domain).to_owned(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => format!("[{ip}]"),
    };
    Some(format!("{}://{host}", url.scheme()))
}

pub fn same_origin(source: &str, target: &str) -> bool {
    origin(source).is_some_and(|source| origin(target).as_deref() == Some(&source))
}

pub fn same_site(source: &str, target: &str) -> bool {
    schemeful_site(source).is_some_and(|source| schemeful_site(target).as_deref() == Some(&source))
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

type ClientBuilderFactory =
    dyn Fn() -> Result<crate::http_backend::ClientBuilder, crate::net::HttpError> + Send + Sync;

#[derive(Default)]
struct HintState {
    origins: Mutex<BTreeMap<String, BTreeSet<String>>>,
    // A pooled connection is driven by the Tokio runtime that created it.
    // Keep one pool, replacing it when an embedder changes runtime.
    client: Mutex<Option<(tokio::runtime::Id, crate::http_backend::Client)>>,
    builder: Option<Arc<ClientBuilderFactory>>,
    upload_client: Mutex<Option<(tokio::runtime::Id, crate::http_backend::Client)>>,
}

impl ClientHints {
    /// Configure a trusted host transport while preserving per-runtime pools and hints.
    /// Manual redirect admission is enforced after this builder factory returns.
    pub fn with_client_builder(
        builder: impl Fn() -> Result<crate::http_backend::ClientBuilder, crate::net::HttpError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self(Arc::new(HintState {
            builder: Some(Arc::new(builder)),
            ..Default::default()
        }))
    }
    pub(crate) fn manual_client(
        &self,
    ) -> Result<crate::http_backend::Client, crate::net::HttpError> {
        let runtime = tokio::runtime::Handle::current().id();
        let mut client = self.0.client.lock().unwrap_or_else(|e| e.into_inner());
        if client.as_ref().is_none_or(|(owner, _)| *owner != runtime) {
            let created = if let Some(builder) = &self.0.builder {
                builder()?
                    .redirect(crate::http_backend::redirect::Policy::none())
                    .build()
                    .map_err(|error| crate::net::HttpError {
                        message: error.to_string(),
                        code: crate::net::ErrorCode::Network,
                    })?
            } else {
                crate::net::build_browser_client()
            };
            *client = Some((runtime, created));
        }
        Ok(client.as_ref().unwrap().1.clone())
    }
    pub(crate) fn upload_client(
        &self,
    ) -> Result<crate::http_backend::Client, crate::net::HttpError> {
        let runtime = tokio::runtime::Handle::current().id();
        let mut client = self
            .0
            .upload_client
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if client.as_ref().is_none_or(|(owner, _)| *owner != runtime) {
            let builder = if let Some(builder) = &self.0.builder {
                builder()?
            } else {
                crate::net::client_builder()
            };
            let created = crate::upload_body::guard(
                builder.redirect(crate::http_backend::redirect::Policy::none()),
            )
            .build()
            .map_err(|error| crate::net::HttpError {
                message: error.to_string(),
                code: crate::net::ErrorCode::Network,
            })?;
            *client = Some((runtime, created));
        }
        Ok(client.as_ref().unwrap().1.clone())
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
        "ect" => "4g".into(),
        "sec-ch-prefers-color-scheme" => "light".into(),
        "sec-ch-prefers-reduced-motion" => "no-preference".into(),
        _ => return None,
    };
    Some(value)
}

/// The image `Accept` for a client whose screenshot raster paints AVIF and/or JPEG XL only
/// when built with those decoders (turbo-surf#43): `None` when it paints both, so the
/// request keeps Chrome's measured image `Accept`; otherwise only the types it paints
/// (PNG/JPEG/GIF/WebP/SVG plus the built ones), so a content-negotiating CDN falls back to
/// a format that does not paint as a placeholder.
pub fn paintable_image_accept(avif: bool, jxl: bool) -> Option<String> {
    if avif && jxl {
        return None;
    }
    let jxl = if jxl { "image/jxl," } else { "" };
    let avif = if avif { "image/avif," } else { "" };
    Some(format!(
        "{jxl}{avif}image/webp,image/png,image/jpeg,image/gif,image/svg+xml,*/*;q=0.8"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paintable_image_accept_drops_only_the_unpainted_types() {
        assert_eq!(paintable_image_accept(true, true), None);
        assert_eq!(
            paintable_image_accept(true, false).as_deref(),
            Some("image/avif,image/webp,image/png,image/jpeg,image/gif,image/svg+xml,*/*;q=0.8")
        );
        assert_eq!(
            paintable_image_accept(false, true).as_deref(),
            Some("image/jxl,image/webp,image/png,image/jpeg,image/gif,image/svg+xml,*/*;q=0.8")
        );
        assert_eq!(
            paintable_image_accept(false, false).as_deref(),
            Some("image/webp,image/png,image/jpeg,image/gif,image/svg+xml,*/*;q=0.8")
        );
    }

    #[test]
    fn browser_pool_does_not_depend_on_a_parked_previous_runtime() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::{Duration, Instant};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, stopped) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut sockets = Vec::new();
            while stopped.try_recv().is_err() && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let mut request = Vec::new();
                        let mut byte = [0];
                        while !request.ends_with(b"\r\n\r\n") {
                            socket.read_exact(&mut byte).unwrap();
                            request.push(byte[0]);
                        }
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: keep-alive\r\n\r\nx").unwrap();
                        sockets.push(socket);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            }
            sockets.len()
        });
        let hints = ClientHints::default();
        let runtime = || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        };
        let first = runtime();
        let second = runtime();
        let fetch = || async {
            tokio::time::timeout(
                Duration::from_secs(2),
                crate::net::fetch_html(
                    &url,
                    crate::net::FetchOptions {
                        client_hints: Some(&hints),
                        allow_non_html: true,
                        ..Default::default()
                    },
                ),
            )
            .await
        };
        let initial = first.block_on(fetch());
        // `first` stays alive but no longer drives its pooled connection.
        let subsequent = second.block_on(fetch());
        stop.send(()).unwrap();
        let connections = server.join().unwrap();
        assert_eq!(initial.unwrap().unwrap().html, "x");
        assert_eq!(subsequent.unwrap().unwrap().html, "x");
        assert_eq!(connections, 2);
    }

    #[test]
    fn storage_site_keys_preserve_schemes_private_suffixes_and_ip_literals() {
        assert_eq!(
            schemeful_site("https://sub.example.co.uk:444/path"),
            Some("https://example.co.uk".into())
        );
        assert_eq!(
            schemeful_site("https://a.github.io"),
            Some("https://a.github.io".into())
        );
        assert_eq!(
            schemeful_site("http://localhost:80"),
            Some("http://localhost".into())
        );
        assert_eq!(
            schemeful_site("http://127.0.0.1:99"),
            Some("http://127.0.0.1".into())
        );
        assert_eq!(
            schemeful_site("http://[::1]:99"),
            Some("http://[::1]".into())
        );
        assert!(!same_site("http://127.0.0.1", "http://127.1.0.1"));
        assert_ne!(
            schemeful_site("http://example.com"),
            schemeful_site("https://example.com")
        );
        assert_eq!(schemeful_site("data:text/html,a"), None);
    }

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

#[cfg(test)]
mod configured_transport_tests {
    use super::ClientHints;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test(flavor = "current_thread")]
    async fn configured_transport_factory_reuses_only_the_current_runtime_pool() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let hints = ClientHints::with_client_builder(move || {
            count.fetch_add(1, Ordering::Relaxed);
            Ok(crate::net::client_builder())
        });
        hints.manual_client().unwrap();
        hints.clone().manual_client().unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let peer = hints.clone();
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async { peer.manual_client().unwrap() });
        })
        .join()
        .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        hints.manual_client().unwrap();
        hints.manual_client().unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configured_transport_errors_do_not_publish_or_poison_a_pool() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let hints = ClientHints::with_client_builder(move || {
            count.fetch_add(1, Ordering::Relaxed);
            crate::net::client_builder_with_trust(b"not a certificate")
        });
        assert!(hints.manual_client().is_err());
        assert!(hints.manual_client().is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}
