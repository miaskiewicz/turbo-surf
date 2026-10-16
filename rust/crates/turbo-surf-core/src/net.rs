//! Networking (port of `src/net.mjs`, SPEC §8): `fetch_html` over reqwest
//! (redirects + gzip/br/deflate for free) plus the hardening the spec calls for:
//! charset sniffing, max body size, content-type gate, and an optional CookieJar
//! round-trip. The pure helpers below are unit-tested offline; `fetch_html`
//! itself is the live-IO seam (covered by the integration suite / harness).

use crate::cache::ResponseCache;
use crate::cookies::CookieJar;
use crate::http_backend as http; // reqwest (rustls) or wreq (BoringSSL); see lib.rs
use crate::http_cache::{CacheMode, Cached, HttpCache, PendingStore, TracePrefix};
use crate::http_cache_ranges::ByteRange;
use crate::request_context::{ClientHints, RequestContext};
use bytes::BytesMut;
use futures_util::StreamExt;
use std::collections::BTreeMap;

const DEFAULT_MAX_BYTES: usize = 8 * 1024 * 1024; // 8 MiB

/// Chrome target of the measured TLS/H2/header profile. Chrome 155 uses the
/// upstream154 TLS/H2 recipe with independently measured155 headers; the base
/// recipe version is reported separately. Rustls has no Chrome transport profile.
pub fn transport_chrome_major() -> Option<u16> {
    #[cfg(feature = "impersonate")]
    return Some(crate::browser::DEFAULT_MAJOR);
    #[cfg(not(feature = "impersonate"))]
    None
}

/// Underlying upstream recipe, distinct from the measured target profile.
pub fn transport_base_chrome_major() -> Option<u16> {
    #[cfg(feature = "impersonate")]
    return Some(154);
    #[cfg(not(feature = "impersonate"))]
    None
}

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
    /// Response bytes before document charset decoding, for fetch Body consumers.
    pub body_bytes: Vec<u8>,
    pub headers: Vec<(String, String)>,
    pub final_url: String,
    pub status: u16,
    pub status_text: String,
    pub redirected: bool,
    pub content_type: String,
}

type BodyStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, HttpError>> + Send>>;
pub type ResponseChunkObserver = std::sync::Arc<dyn Fn(&[u8]) + Send + Sync>;

/// A live response body, optionally backed by an owner-cancellable native cache fill.
pub struct ResponseBody {
    stream: Option<BodyStream>,
    received: usize,
    max_bytes: usize,
    trace_id: u64,
    url: String,
    status: u16,
    content_type: String,
    trace_body: Option<Vec<u8>>,
    trace_prefix: usize,
    trace_skip_prefix: usize,
    cache_store: Option<PendingStore>,
    background: Option<std::sync::Arc<BackgroundBody>>,
}
struct BackgroundBody {
    state: std::sync::Mutex<BackgroundState>,
    ready: tokio::sync::Notify,
    cancel: tokio::sync::watch::Sender<bool>,
}
#[derive(Default)]
struct BackgroundState {
    chunks: std::collections::VecDeque<Vec<u8>>,
    done: bool,
    error: Option<HttpError>,
    failed: bool,
    detached: bool,
}
impl BackgroundBody {
    fn push(&self, bytes: &[u8]) {
        const CHUNK: usize = 64 * 1024;
        let mut state = self.state.lock().unwrap();
        if state.detached {
            return;
        }
        let mut remaining = bytes;
        while !remaining.is_empty() {
            if state.chunks.back().is_none_or(|chunk| chunk.len() == CHUNK) {
                state.chunks.push_back(Vec::new());
            }
            let tail = state.chunks.back_mut().unwrap();
            let count = remaining.len().min(CHUNK - tail.len());
            tail.extend_from_slice(&remaining[..count]);
            remaining = &remaining[count..];
        }
        drop(state);
        self.ready.notify_one();
    }
    fn detach(&self) {
        let mut state = self.state.lock().unwrap();
        state.detached = true;
        state.chunks.clear();
    }
}
impl ResponseBody {
    /// Cache-eligible responses continue downloading without guest demand. The
    /// owner watch remains external: closing it cancels even a collected body.
    pub fn background_cache(self, owner: tokio::sync::watch::Receiver<bool>) -> Self {
        self.background_cache_with_observer(owner, None)
    }
    pub fn background_cache_with_observer(
        self,
        mut owner: tokio::sync::watch::Receiver<bool>,
        observer: Option<ResponseChunkObserver>,
    ) -> Self {
        if self.cache_store.is_none() {
            return self;
        }
        let (cancel, mut canceled) = tokio::sync::watch::channel(false);
        let background = std::sync::Arc::new(BackgroundBody {
            state: std::sync::Mutex::new(BackgroundState::default()),
            ready: tokio::sync::Notify::new(),
            cancel,
        });
        let queued = background.clone();
        let stream = futures_util::stream::unfold(queued, |queued| async move {
            loop {
                let ready = queued.ready.notified();
                {
                    let mut state = queued.state.lock().unwrap();
                    if let Some(bytes) = state.chunks.pop_front() {
                        return Some((Ok(bytes::Bytes::from(bytes)), queued.clone()));
                    }
                    if state.done {
                        return state.error.take().map(|error| (Err(error), queued.clone()));
                    }
                }
                ready.await;
            }
        });
        let consumer = Self {
            stream: Some(Box::pin(stream)),
            received: 0,
            max_bytes: self.max_bytes,
            trace_id: self.trace_id,
            url: self.url.clone(),
            status: self.status,
            content_type: self.content_type.clone(),
            trace_body: None,
            trace_prefix: self.trace_prefix,
            trace_skip_prefix: 0,
            cache_store: None,
            background: Some(background.clone()),
        };
        tokio::spawn(async move {
            let mut transport = self;
            let error = loop {
                let chunk = if *owner.borrow() || *canceled.borrow() {
                    None
                } else {
                    tokio::select! {
                        biased;
                        _ = owner.changed() => None,
                        _ = canceled.changed() => None,
                        chunk = transport.next_chunk() => Some(chunk),
                    }
                };
                match chunk {
                    Some(Ok(Some(bytes))) => {
                        if let Some(observer) = &observer {
                            observer(&bytes);
                        }
                        background.push(&bytes);
                    }
                    Some(Ok(None)) => break None,
                    Some(Err(error)) => break Some(error),
                    None => {
                        transport.cancel();
                        break Some(HttpError::new("Response body canceled", ErrorCode::Network));
                    }
                }
            };
            let mut state = background.state.lock().unwrap();
            state.done = true;
            state.failed = error.is_some();
            state.error = error;
            drop(state);
            background.ready.notify_one();
        });
        consumer
    }
    /// Collection releases queued guest bytes while the native cache fill keeps
    /// its creation-owner cancellation watch. No JS or IO occurs here.
    pub fn detach_cache_consumer(&mut self) -> bool {
        let Some(background) = &self.background else {
            return false;
        };
        background.detach();
        true
    }
    pub fn background_complete(&self) -> bool {
        self.background
            .as_ref()
            .is_some_and(|body| body.state.lock().unwrap().done)
    }
    pub fn has_background_cache(&self) -> bool {
        self.background.is_some()
    }
    /// Completed transport status for host logging; None means still receiving.
    pub fn background_status(&self) -> Option<bool> {
        let state = self.background.as_ref()?.state.lock().unwrap();
        state.done.then_some(!state.failed)
    }
    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        let Some(stream) = self.stream.as_mut() else {
            return Ok(None);
        };
        let chunk = loop {
            match stream.next().await {
                // HTTP/2 can expose empty DATA frames. They carry no body
                // bytes and cannot be enqueued into a Web byte stream.
                Some(Ok(chunk)) if chunk.is_empty() => continue,
                Some(Ok(chunk)) => break chunk,
                Some(Err(error)) => {
                    self.cancel();
                    return Err(error);
                }
                None => {
                    self.stream = None;
                    self.finish_trace(false);
                    if let Some(pending) = self.cache_store.take() {
                        pending.cache.store(pending.entry);
                    }
                    return Ok(None);
                }
            }
        };
        if chunk.len() > self.max_bytes.saturating_sub(self.received) {
            self.cancel();
            return Err(HttpError::new(
                format!("body exceeds maxBytes (> {})", self.max_bytes),
                ErrorCode::BodyTooLarge,
            ));
        }
        self.received += chunk.len();
        if let Some(pending) = self.cache_store.as_mut() {
            let skip = pending.skip_prefix.min(chunk.len());
            pending.skip_prefix -= skip;
            pending.entry.body.extend_from_slice(&chunk[skip..]);
        }
        if let Some(body) = self.trace_body.as_mut() {
            let skip = self.trace_skip_prefix.min(chunk.len());
            self.trace_skip_prefix -= skip;
            body.extend_from_slice(&chunk[skip..])
        }
        Ok(Some(chunk.to_vec()))
    }
    /// Cancel the retained transport and discard an incomplete cache entry.
    pub fn cancel(&mut self) {
        if let Some(background) = &self.background {
            background.cancel.send_replace(true);
            background.detach();
        }
        self.stream = None;
        self.cache_store = None;
        self.finish_trace(true);
    }
    fn finish_trace(&mut self, incomplete: bool) {
        let Some(bytes) = self.trace_body.take() else {
            return;
        };
        let charset = detect_charset(&self.content_type, &bytes[..bytes.len().min(1024)]);
        let html = decode(&bytes, &charset);
        crate::http_trace::record(
            || serde_json::json!({"event":"body","id":self.trace_id,"url":self.url,"status":self.status,"content_type":self.content_type,"body":html,"streamed":true,"incomplete":incomplete,"cache_prefix_bytes":self.trace_prefix}),
        );
    }
}
impl Drop for ResponseBody {
    fn drop(&mut self) {
        if let Some(background) = &self.background {
            background.detach();
        }
        self.finish_trace(true)
    }
}
/// Response metadata becomes available before the body has finished downloading.
pub struct StreamingResult {
    pub body: ResponseBody,
    pub headers: Vec<(String, String)>,
    pub final_url: String,
    pub status: u16,
    pub status_text: String,
    pub redirected: bool,
    pub content_type: String,
}

pub type RequestPolicy<'a> = dyn Fn(&str, bool) -> bool + Send + Sync + 'a;
pub type ResponseSideEffects<'a> =
    dyn Fn(&RequestContext, &str, &[(String, String)]) + Send + Sync + 'a;
pub type ResponsePolicy<'a> =
    dyn Fn(&RequestContext, &str, &[(String, String)]) -> bool + Send + Sync + 'a;
#[derive(Clone)]
struct NetworkResponseEffects {
    url: String,
    headers: Vec<(String, String)>,
}

pub type BeforeRequestFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), HttpError>> + Send>>;
pub type BeforeRequest<'a> = dyn Fn(RequestContext, String, String, BTreeMap<String, String>) -> BeforeRequestFuture
    + Send
    + Sync
    + 'a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectMode {
    Follow,
    Error,
    Manual,
}

#[derive(Default)]
pub struct FetchOptions<'a> {
    pub headers: BTreeMap<String, String>,
    /// Browser metadata for this request. Unspecified requests are top-level
    /// navigations; script/fetch/image callers should identify their context.
    pub context: Option<RequestContext>,
    /// A native embedder policy checked before contacting every redirect target.
    pub request_policy: Option<&'a RequestPolicy<'a>>,
    /// A native embedder response policy checked before following redirects.
    pub response_policy: Option<&'a ResponsePolicy<'a>>,
    /// Pure native effects after credential admission, before guest response policy.
    /// Raw network headers, including revalidation; cached headers never replay it.
    pub response_side_effects: Option<&'a ResponseSideEffects<'a>>,
    /// Async native authorization before each request, using the rewritten hop's
    /// context, URL, method and authored headers (for example CORS preflight).
    pub before_request: Option<&'a BeforeRequest<'a>>,
    /// Origin-scoped Accept-CH opt-ins shared by this browsing session.
    pub client_hints: Option<&'a ClientHints>,
    pub method: Option<String>,
    pub body: Option<String>,
    /// Binary payload wins over the legacy UTF-8 string when present.
    pub body_bytes: Option<Vec<u8>>,
    /// One-shot HTTP/2 request body; never replayed by a redirect or retry.
    pub upload_body: Option<crate::upload_body::UploadBody>,
    pub max_bytes: Option<usize>,
    pub allow_non_html: bool,
    pub max_redirects: Option<usize>,
    /// Explicit Fetch redirect semantics; None preserves legacy max_redirects behavior.
    pub redirect: Option<RedirectMode>,
    pub jar: Option<&'a mut CookieJar>,
    pub cache: Option<&'a mut ResponseCache>,
    /// Browser HTTP cache, independent of the crawler's legacy validator cache.
    pub http_cache: Option<&'a HttpCache>,
    pub cache_mode: CacheMode,
    /// Native top-level site / frame site / anonymous-context partition.
    pub cache_partition: String,
    /// Shared client for connection reuse (see [`build_client`]); built per-call
    /// when `None`.
    pub client: Option<&'a http::Client>,
    /// Fingerprint identity for the default (rustls) header set. `None` uses
    /// [`crate::fingerprint::default_profile`] (the fixed current-stable / macOS set);
    /// pass [`crate::fingerprint::select`]`(key)` to rotate per client. This per-call
    /// arg is ignored on the `impersonate` path, where wreq's emulation owns the
    /// header set/order and `default_profile`'s UA + `sec-ch-ua` are pinned on top
    /// (see [`emulate`]).
    pub profile: Option<&'a crate::fingerprint::Profile>,
    /// Seed well-known consent-wall cookies for the request host (see
    /// [`crate::consent`]) so JS-gated "before you continue" interstitials are
    /// dismissed and the real, server-rendered page is returned. Off by default.
    pub bypass_consent: bool,
    /// Override the seeded `SOCS` consent value when `bypass_consent` is on
    /// (param-driven; `None` = [`crate::consent::DEFAULT_SOCS`]). An empty string
    /// seeds NO `SOCS`, so the raw consent-gated response is returned even with
    /// `bypass_consent` on — useful when a caller wants the real interstitial to run
    /// its own Accept-all handshake rather than shortcut it with a synthetic value.
    pub consent_socs: Option<String>,
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

fn build_headers(url: &str, method: &str, opts: &FetchOptions) -> BTreeMap<String, String> {
    let mut h = BTreeMap::new();
    let default_profile = crate::fingerprint::default_profile();
    #[cfg(feature = "impersonate")]
    let profile = &default_profile;
    #[cfg(not(feature = "impersonate"))]
    let profile = opts.profile.unwrap_or(&default_profile);
    let default_context = RequestContext::default();
    let context = opts.context.as_ref().unwrap_or(&default_context);
    h.insert("user-agent".into(), profile.user_agent.clone());
    h.insert("accept-language".into(), profile.accept_language.into());
    h.extend(context.headers(url, method));
    let no_opt_ins = ClientHints::default();
    h.extend(
        opts.client_hints
            .unwrap_or(&no_opt_ins)
            .headers(url, context, profile),
    );
    for (k, v) in &opts.headers {
        h.insert(k.to_ascii_lowercase(), v.clone());
    }
    if context.sends_credentials(url) {
        if let Some(jar) = &opts.jar {
            let cookie = jar.cookie_header_for_request(url, opts.now, context, method);
            if !cookie.is_empty() {
                h.insert("cookie".into(), cookie);
            }
        }
    }
    if context.credentialless && !context.sends_credentials(url) {
        h.remove("cookie");
    }
    if opts.bypass_consent && context.sends_credentials(url) {
        seed_consent_cookies(&mut h, url, opts.consent_socs.as_deref());
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
fn seed_consent_cookies(h: &mut BTreeMap<String, String>, url: &str, socs: Option<&str>) {
    let host = crate::url::host_of(url).unwrap_or_default();
    let extra = crate::consent::cookies_for_host_with(&host, socs);
    if extra.is_empty() {
        return;
    }
    let mut cookie = h.get("cookie").cloned().unwrap_or_default();
    for (name, value) in &extra {
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
//
// The measured Chrome 155 TLS/H2 capabilities reuse upstream154; the identity
// and navigation header order are pinned to fingerprint::default_profile(). This
// is an emulation, not proof of current Chrome wire parity. Validate the deployed
// build with the fingerprint tests, especially after a Chrome/backend upgrade.
// Per-call profile overrides are not applied on this backend.
fn emulate(builder: http::ClientBuilder) -> http::ClientBuilder {
    #[cfg(feature = "impersonate")]
    let builder = {
        use wreq::IntoEmulation;
        let profile = crate::fingerprint::default_profile();
        // Resolve the wreq-util Chrome 154 profile to a concrete `Emulation` so its
        // header set, header ORDER and TLS options can be tuned to match a real
        // Chrome *cold* top-level navigation before it's applied.
        let mut emulation = wreq_util::Emulation::builder()
            .profile(wreq_util::Profile::Chrome154)
            .platform(wreq_util::Platform::MacOS)
            .build()
            .into_emulation();

        // Request metadata comes from build_headers, including the negotiated
        // hint set. Client defaults must not leak navigation headers onto script
        // and fetch requests; keep only transport-owned defaults here.
        emulation.headers.clear();
        let mut put = |k: &'static str, v: &str| {
            if let Ok(val) = v.parse() {
                emulation.headers.insert(k, val);
            }
        };
        // Chrome's Accept-Encoding spelling + order (`gzip, deflate, br, zstd`).
        // Left unset, wreq lets tower-http synthesize `zstd,gzip,deflate,br` (its
        // own order, no spaces) — a value tell. Setting it here wins because
        // tower-http only fills Accept-Encoding when the header is *vacant*.
        put("accept-encoding", "gzip, deflate, br, zstd");

        // Pin the exact top-level-navigation header ORDER for the profile's Chrome
        // version (order is itself a fingerprint, and Chrome reorders it across
        // releases — e.g. 154 hoists `accept-language` ahead of `accept`). The order
        // comes from the per-version `crate::browser` registry via
        // `profile.header_order`. wreq emits the named headers first, in this order,
        // via `OrigHeaderMap`, then appends any others (cache validators, extra caller
        // headers); a header absent from a request is skipped. `cookie` sits in
        // Chrome's slot so a jar/consent cookie lands correctly when present.
        let mut order = http::header::OrigHeaderMap::new();
        for name in profile.header_order {
            order.insert(*name);
        }
        emulation.orig_headers = order;

        // The upstream Chrome 154 profile owns signature-algorithm GREASE and
        // real trust-anchor IDs. Do not replace its Chromium root list with the
        // empty extension used by the former compatibility patch.
        builder
            .emulation(emulation)
            // Override AFTER emulation (wreq applies the profile immediately, so
            // later fine-tuning wins) to use the configured full patch version
            // from the browser registry.
            .user_agent(profile.user_agent.as_str())
    };
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
    client_builder()
        .build()
        .expect("HTTP client build (TLS backend init)")
}

/// Configured native transport builder for host-owned trust and pooling options.
/// Use this seam when an embedder needs a private certificate store; normal
/// browsing retains the configured backend's certificate verification.
pub fn client_builder() -> http::ClientBuilder {
    emulate(
        http::Client::builder()
            .redirect(http::redirect::Policy::limited(20))
            .pool_idle_timeout(std::time::Duration::from_secs(90)),
    )
}

/// Native report client: no redirects or browserless navigation header defaults.
#[derive(Clone)]
pub struct ReportClient {
    legacy: http::Client,
    #[cfg(not(feature = "impersonate"))]
    modern: ModernReportClient,
}
#[cfg(not(feature = "impersonate"))]
type ModernReportClient = hyper_util::client::legacy::Client<
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
    http_body_util::Full<bytes::Bytes>,
>;
#[cfg(not(feature = "impersonate"))]
fn report_https_client(der: Option<&[u8]>) -> Result<ModernReportClient, HttpError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(der) = der {
        roots
            .add(rustls::pki_types::CertificateDer::from(der.to_vec()))
            .map_err(|error| err(error, ErrorCode::Network))?;
    }
    let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| err(error, ErrorCode::Network))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .build();
    Ok(
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(connector),
    )
}
pub fn build_report_client() -> ReportClient {
    ReportClient {
        legacy: build_browser_client(),
        #[cfg(not(feature = "impersonate"))]
        modern: report_https_client(None).expect("native report TLS roots"),
    }
}
/// Native transport builder with an embedder-supplied DER trust anchor.
/// Certificate verification remains enabled; the factory is never guest-visible.
pub fn client_builder_with_trust(der: &[u8]) -> Result<http::ClientBuilder, HttpError> {
    let builder = client_builder().redirect(http::redirect::Policy::none());
    #[cfg(feature = "impersonate")]
    let builder = builder.tls_cert_store(
        http::tls::trust::CertStore::builder()
            .add_der_cert(der)
            .build()
            .map_err(|error| err(error, ErrorCode::Network))?,
    );
    #[cfg(not(feature = "impersonate"))]
    let builder = builder.add_root_certificate(
        http::Certificate::from_der(der).map_err(|error| err(error, ErrorCode::Network))?,
    );
    Ok(builder)
}
/// Report client with an embedder-supplied DER trust anchor; verification remains enabled.
pub fn report_client_with_trust(der: &[u8]) -> Result<ReportClient, HttpError> {
    Ok(ReportClient {
        legacy: client_builder_with_trust(der)?
            .build()
            .map_err(|error| err(error, ErrorCode::Network))?,
        #[cfg(not(feature = "impersonate"))]
        modern: report_https_client(Some(der))?,
    })
}
pub struct ReportResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}
/// Send raw native bytes and discard the response at header arrival. Modern
/// reports use controlled Hyper headers because reqwest always supplies Accept.
pub async fn send_report_request(
    client: &ReportClient,
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: Vec<u8>,
    modern: bool,
) -> Result<ReportResponse, HttpError> {
    let trace_id = crate::http_trace::next_id();
    crate::http_trace::record(
        || serde_json::json!({"event":"request","id":trace_id.0,"url":url,"method":method,"explicit_headers":headers,"body_bytes":body,"destination":"report","headerScope":"native report headers; transport-added headers absent"}),
    );
    #[cfg(feature = "impersonate")]
    let _ = modern;
    #[cfg(not(feature = "impersonate"))]
    if modern {
        let mut request = ::http::Request::builder().method(method).uri(url);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let request = request
            .body(http_body_util::Full::new(bytes::Bytes::from(body)))
            .map_err(|error| err(error, ErrorCode::Network))?;
        let response = client
            .modern
            .request(request)
            .await
            .map_err(|error| err(error, ErrorCode::Network))?;
        crate::http_trace::record(
            || serde_json::json!({"event":"response","id":trace_id.0,"url":url,"status":response.status().as_u16(),"protocol":format!("{:?}",response.version()),"headers":response.headers().iter().map(|(key,value)|(key.as_str(),value.to_str().unwrap_or("<binary>"))).collect::<Vec<_>>() }),
        );
        return Ok(ReportResponse {
            status: response.status().as_u16(),
            headers: response
                .headers()
                .iter()
                .map(|(key, value)| {
                    (
                        key.to_string(),
                        value.to_str().unwrap_or_default().to_string(),
                    )
                })
                .collect(),
        });
    }
    let method_value = http::Method::from_bytes(method.as_bytes())
        .map_err(|error| err(error, ErrorCode::Network))?;
    let response = apply_headers(client.legacy.request(method_value, url), headers)
        .body(body)
        .send()
        .await
        .map_err(|error| err(error, ErrorCode::Network))?;
    crate::http_trace::record(
        || serde_json::json!({"event":"response","id":trace_id.0,"url":url,"final_url":final_url(&response),"status":response.status().as_u16(),"protocol":format!("{:?}",response.version()),"headers":response.headers().iter().map(|(key,value)|(key.as_str(),value.to_str().unwrap_or("<binary>"))).collect::<Vec<_>>() }),
    );
    Ok(ReportResponse {
        status: response.status().as_u16(),
        headers: response
            .headers()
            .iter()
            .map(|(key, value)| {
                (
                    key.to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect(),
    })
}

pub(crate) fn build_browser_client() -> http::Client {
    emulate(
        http::Client::builder()
            .redirect(http::redirect::Policy::none())
            .pool_idle_timeout(std::time::Duration::from_secs(90)),
    )
    .build()
    .expect("browser HTTP client build (TLS backend init)")
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
    body_bytes: &Option<Vec<u8>>,
    upload: Option<crate::upload_body::UploadBody>,
) -> Result<http::Response, HttpError> {
    let m = http::Method::from_bytes(method.as_bytes()).map_err(|e| err(e, ErrorCode::Network))?;
    let mut req = apply_headers(cl.request(m, url), headers);
    let streaming_upload = upload.is_some();
    if let Some(upload) = upload {
        #[cfg(not(feature = "impersonate"))]
        {
            req = req.version(http::Version::HTTP_2);
        }
        req = req.body(upload.into_body()?);
    } else if let Some(bytes) = body_bytes {
        req = req.body(bytes.clone());
    } else if let Some(b) = body {
        req = req.body(b.clone());
    }
    // Include the URL in transport errors — reqwest's "builder error" (e.g. a
    // relative/schemeless URL) is otherwise undebuggable.
    let trace_id = crate::http_trace::next_id();
    crate::http_trace::record(|| {
        serde_json::json!({
            "event": "request", "id":trace_id.0, "url": url, "method": method,
            "explicit_headers": headers, "body": body, "body_bytes": body_bytes,
            "body_streaming": streaming_upload,
            "headerScope": "application headers including context and hints; transport-added headers absent",
        })
    });
    let mut result = req.send().await;
    if trace_id.0 != 0 {
        if let Ok(res) = &mut result {
            res.extensions_mut().insert(trace_id);
        }
    }
    crate::http_trace::record(|| match &result {
        Ok(res) => serde_json::json!({
            "event": "response", "id":trace_id.0, "url": url, "final_url": final_url(res),
            "status": res.status().as_u16(),
            "protocol":format!("{:?}",res.version()),
            "headers": res.headers().iter().map(|(k, v)|
                (k.as_str(), v.to_str().unwrap_or("<binary>"))).collect::<Vec<_>>(),
        }),
        Err(e) => {
            serde_json::json!({ "event": "error", "id":trace_id.0, "url": url, "error": e.to_string() })
        }
    });
    result.map_err(|e| err(format!("{e} (url: {url})"), ErrorCode::Network))
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
    if let Some(hints) = opts.client_hints {
        let accept_ch = list_header(res, "accept-ch");
        let sends_credentials = opts
            .context
            .as_ref()
            .is_none_or(|context| context.sends_credentials(final_url));
        let clear = sends_credentials
            && list_header(res, "clear-site-data")
                .unwrap_or_default()
                .split(',')
                .any(|item| matches!(item.trim(), "\"clientHints\"" | "\"cache\"" | "\"*\""));
        hints.observe(final_url, accept_ch.as_deref(), clear);
    }
    if opts
        .context
        .as_ref()
        .is_some_and(|ctx| !ctx.sends_credentials(final_url))
    {
        return;
    }
    if let Some(cache) = opts.http_cache {
        let clear = list_header(res, "clear-site-data").unwrap_or_default();
        cache.clear_response(final_url, &clear);
    }
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

fn status_text(response: &http::Response) -> String {
    if let Some(cached) = response.extensions().get::<Cached>() {
        return cached.0.clone();
    }
    if !matches!(
        response.version(),
        http::Version::HTTP_09 | http::Version::HTTP_10 | http::Version::HTTP_11
    ) {
        return String::new();
    }
    #[cfg(feature = "impersonate")]
    type Phrase = wreq_proto::ext::ReasonPhrase;
    #[cfg(not(feature = "impersonate"))]
    type Phrase = hyper::ext::ReasonPhrase;
    if let Some(phrase) = response.extensions().get::<Phrase>() {
        return phrase
            .as_ref()
            .iter()
            .map(|byte| char::from(*byte))
            .collect();
    }
    response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_owned()
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
    let cache_store = res.extensions().get::<PendingStore>().cloned();
    let trace_prefix = res
        .extensions()
        .get::<TracePrefix>()
        .map_or(0, |prefix| prefix.0);
    let status = res.status().as_u16();
    let status_text = status_text(&res);
    let headers = res
        .headers()
        .iter()
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (key.as_str().to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();
    let trace_id = res
        .extensions()
        .get::<crate::http_trace::RequestId>()
        .map(|id| id.0)
        .unwrap_or(0);
    // 304 Not Modified → reuse the cached body (server sent none).
    if status == 304 {
        if let Some(cache) = &opts.cache {
            let html = cache.body(&final_url);
            return Ok(FetchResult {
                body_bytes: html.as_bytes().to_vec(),
                headers,
                html,
                final_url,
                status,
                status_text,
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
    if let Some(mut pending) = cache_store {
        pending.entry.body = bytes[pending.skip_prefix.min(bytes.len())..].to_vec();
        pending.cache.store(pending.entry);
    }
    let charset = detect_charset(&ct, &bytes[..bytes.len().min(1024)]);
    let html = decode(&bytes, &charset);
    crate::http_trace::record(|| {
        serde_json::json!({
            "event": "body", "id":trace_id, "url": final_url, "status": status,
            "content_type": ct, "body": if trace_prefix==0 { html.clone() } else { decode(&bytes[trace_prefix.min(bytes.len())..],&charset) }, "cache_prefix_bytes":trace_prefix,
        })
    });
    if let Some(cache) = opts.cache.as_mut() {
        cache.store(&final_url, etag, last_modified, &html);
    }
    Ok(FetchResult {
        body_bytes: bytes.to_vec(),
        headers,
        html,
        final_url,
        status,
        status_text,
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

// List-valued headers can arrive as multiple field lines (HTTP/2 keeps them
// separate). HeaderMap::get returns only the first, losing later hint opt-ins.
fn list_header(res: &http::Response, name: &str) -> Option<String> {
    let values: Vec<&str> = res
        .headers()
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

// State threaded through the manual-follow loop.
struct Hop {
    url: String,
    method: String,
    body: Option<String>,
    body_bytes: Option<Vec<u8>>,
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
    let (res, final_url, redirected) = follow_manually_response(opts, start, max_redirects).await?;
    finish(opts, res, final_url, redirected, max_bytes).await
}

struct RangeFill {
    entry: crate::http_cache::Entry,
    range: ByteRange,
    prefix: Vec<u8>,
    is_range: bool,
    validator: String,
}
fn finish_range_response(
    opts: &FetchOptions<'_>,
    hop: &Hop,
    headers: &BTreeMap<String, String>,
    response: http::Response,
    fill: RangeFill,
    generation: u64,
) -> Result<http::Response, HttpError> {
    let actual_headers = response
        .headers()
        .iter()
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (key.as_str().to_owned(), value.to_owned()))
        })
        .collect::<Vec<_>>();
    let context = opts.context.clone().unwrap_or_default();
    if opts
        .response_policy
        .is_some_and(|allowed| !allowed(&context, &hop.url, &actual_headers))
    {
        return Err(HttpError::new(
            "Response blocked by native document policy",
            ErrorCode::Network,
        ));
    }
    let expected_start = fill.range.start + fill.prefix.len();
    let span = ByteRange::content(&header_value(&response, "content-range"));
    if header_value(&response, "etag") != fill.validator
        || span.is_none_or(|(range, total)| {
            range.start != expected_start
                || range.end != fill.range.end
                || total != fill.entry.selected_range(None).unwrap().end
        })
    {
        return Err(HttpError::new(
            "Invalid conditional range response",
            ErrorCode::Network,
        ));
    }
    let raw_effects = response
        .extensions()
        .get::<NetworkResponseEffects>()
        .cloned();
    let trace_id = response
        .extensions()
        .get::<crate::http_trace::RequestId>()
        .cloned();
    let mut pending = opts.http_cache.and_then(|cache| {
        cache.pending(
            &opts.cache_partition,
            &hop.url,
            headers,
            &response,
            status_text(&response),
            generation,
        )
    });
    if let Some(pending) = &mut pending {
        pending.skip_prefix = fill.prefix.len();
    }
    let mut entry = fill.entry.response_metadata(fill.range, fill.is_range);
    if fill.prefix.is_empty() {
        entry.headers = actual_headers;
        entry = entry.response_metadata(fill.range, fill.is_range);
    }
    let prefix_len = fill.prefix.len();
    let prefix = (!fill.prefix.is_empty()).then_some(Ok::<bytes::Bytes, http::Error>(
        bytes::Bytes::from(fill.prefix),
    ));
    let stream = futures_util::stream::iter(prefix).chain(response.bytes_stream());
    let mut response = entry.response_body(http::Body::wrap_stream(stream));
    response.extensions_mut().insert(TracePrefix(prefix_len));
    if let Some(pending) = pending {
        response.extensions_mut().insert(pending);
    }
    if let Some(effects) = raw_effects {
        response.extensions_mut().insert(effects);
    }
    if let Some(trace_id) = trace_id {
        response.extensions_mut().insert(trace_id);
    }
    Ok(response)
}

async fn cached_request(
    opts: &mut FetchOptions<'_>,
    hop: &Hop,
    client: &http::Client,
) -> Result<http::Response, HttpError> {
    if let Some(before) = opts.before_request {
        before(
            opts.context.clone().unwrap_or_default(),
            hop.url.clone(),
            hop.method.clone(),
            opts.headers.clone(),
        )
        .await?;
    }
    let mut headers = build_headers(&hop.url, &hop.method, opts);
    let cache = opts.http_cache;
    let generation = cache.map_or(0, HttpCache::generation);
    let eligible = matches!(hop.method.as_str(), "GET" | "HEAD");
    let range_header = (hop.method == "GET")
        .then(|| headers.get("range").cloned())
        .flatten();
    let mode = if opts.http_cache.is_some()
        && opts.cache_mode == CacheMode::Default
        && [
            "if-none-match",
            "if-modified-since",
            "if-unmodified-since",
            "if-match",
            "if-range",
        ]
        .iter()
        .any(|name| headers.contains_key(*name))
    {
        CacheMode::NoStore
    } else {
        opts.cache_mode
    };
    if mode == CacheMode::OnlyIfCached && (!eligible || range_header.is_some()) {
        return Err(HttpError::new("HTTP cache miss", ErrorCode::Network));
    }
    let candidate = if eligible && !matches!(mode, CacheMode::NoStore | CacheMode::Reload) {
        cache.and_then(|cache| cache.lookup(&opts.cache_partition, &hop.url, &headers))
    } else {
        None
    };
    let reusable = candidate.as_ref().and_then(|(entry, matches)| {
        (*matches
            && (matches!(mode, CacheMode::ForceCache | CacheMode::OnlyIfCached)
                || mode == CacheMode::Default && entry.fresh())
            && !(mode == CacheMode::OnlyIfCached && entry.sparse()))
        .then(|| entry.range_response(range_header.as_deref()))
        .flatten()
    });
    let fill = candidate.as_ref().and_then(|(entry, matches)| {
        if !*matches || !entry.sparse() || !entry.fresh() && mode == CacheMode::Default {
            return None;
        }
        let range = entry.selected_range(range_header.as_deref())?;
        let prefix = entry.prefix(range);
        (prefix.len() < range.end - range.start).then_some(RangeFill {
            entry: entry.clone(),
            range,
            prefix,
            is_range: range_header.is_some(),
            validator: entry.strong_validator()?,
        })
    });
    let mut res = if let Some(mut entry) = reusable {
        if hop.method == "HEAD" {
            entry.body.clear();
        }
        entry.response()
    } else {
        if mode == CacheMode::OnlyIfCached {
            return Err(HttpError::new("HTTP cache miss", ErrorCode::Network));
        }
        if let Some(fill) = &fill {
            headers.insert(
                "range".into(),
                format!(
                    "bytes={}-{}",
                    fill.range.start + fill.prefix.len(),
                    fill.range.end - 1
                ),
            );
            headers.insert("if-range".into(), fill.validator.clone());
        } else if range_header.is_none() {
            if let Some((entry, _)) = &candidate {
                entry.validators(&mut headers);
            }
        }
        match mode {
            CacheMode::Reload | CacheMode::NoStore => {
                headers
                    .entry("cache-control".into())
                    .or_insert("no-cache".into());
                headers.entry("pragma".into()).or_insert("no-cache".into());
            }
            CacheMode::NoCache => {
                headers
                    .entry("cache-control".into())
                    .or_insert("max-age=0".into());
            }
            _ => {}
        }
        let mut response = send(
            client,
            &hop.method,
            &hop.url,
            &headers,
            &hop.body,
            &hop.body_bytes,
            opts.upload_body.take(),
        )
        .await?;
        if opts.response_side_effects.is_some() {
            let effects = NetworkResponseEffects {
                url: final_url(&response),
                headers: response
                    .headers()
                    .iter()
                    .filter_map(|(key, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|value| (key.as_str().to_owned(), value.to_owned()))
                    })
                    .collect(),
            };
            response.extensions_mut().insert(effects);
        }
        let raw_effects = response
            .extensions()
            .get::<NetworkResponseEffects>()
            .cloned();
        ingest_set_cookie(opts, &response, &hop.url);
        if let Some(fill) = fill.filter(|_| response.status().as_u16() == 206) {
            return finish_range_response(opts, hop, &headers, response, fill, generation);
        }
        if response.status().as_u16() == 304 {
            if let Some((mut entry, _)) = candidate {
                let response_headers = response
                    .headers()
                    .iter()
                    .filter_map(|(key, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|value| (key.as_str().to_owned(), value.to_owned()))
                    })
                    .collect::<Vec<_>>();
                entry.merge(&response_headers, &headers);
                let mut prepared =
                    entry
                        .range_response(range_header.as_deref())
                        .ok_or_else(|| {
                            HttpError::new(
                                "Incomplete cached response for revalidation",
                                ErrorCode::Network,
                            )
                        })?;
                if let Some(cache) = cache {
                    cache.store(entry);
                }
                if hop.method == "HEAD" {
                    prepared.body.clear();
                }
                let mut response = prepared.response();
                if let Some(effects) = raw_effects {
                    response.extensions_mut().insert(effects);
                }
                response
            } else {
                response
            }
        } else {
            response
        }
    };
    if !matches!(hop.method.as_str(), "GET" | "HEAD" | "OPTIONS" | "TRACE")
        && res.status().as_u16() < 400
    {
        if let Some(cache) = cache {
            cache.invalidate(&opts.cache_partition, &hop.url);
        }
    }
    if hop.method == "GET"
        && eligible
        && mode != CacheMode::NoStore
        && res.extensions().get::<Cached>().is_none()
    {
        if let Some(pending) = cache.and_then(|cache| {
            cache.pending(
                &opts.cache_partition,
                &hop.url,
                &headers,
                &res,
                status_text(&res),
                generation,
            )
        }) {
            res.extensions_mut().insert(pending);
        }
    }
    Ok(res)
}

async fn follow_manually_response(
    opts: &mut FetchOptions<'_>,
    start: &str,
    max_redirects: usize,
) -> Result<(http::Response, String, bool), HttpError> {
    let streaming_upload = opts.upload_body.is_some();
    if streaming_upload && !start.starts_with("https:") {
        return Err(HttpError::new(
            "Streaming requests require secure HTTP/2",
            ErrorCode::Network,
        ));
    }
    let cl = match opts.client_hints {
        Some(hints) if streaming_upload => hints.upload_client()?,
        Some(hints) => hints.manual_client()?,
        None if streaming_upload => {
            crate::upload_body::guard(client_builder().redirect(http::redirect::Policy::none()))
                .build()
                .map_err(|e| err(e, ErrorCode::Network))?
        }
        None => client(http::redirect::Policy::none()).map_err(|e| err(e, ErrorCode::Network))?,
    };
    let mut hop = Hop {
        url: start.to_string(),
        method: opts.method.clone().unwrap_or_else(|| "GET".to_string()),
        body: opts.body.clone(),
        body_bytes: opts.body_bytes.clone(),
    };
    for redirects in 0.. {
        if opts
            .request_policy
            .is_some_and(|allowed| !allowed(&hop.url, redirects > 0))
        {
            return Err(HttpError::new(
                "Request blocked by native document policy",
                ErrorCode::Network,
            ));
        }
        if opts.context.as_ref().is_some_and(|context| {
            context.mode == crate::request_context::RequestMode::SameOrigin
                && (context.opaque_origin
                    || context.origin_tainted
                    || context.source.as_deref().is_none_or(|source| {
                        !crate::request_context::same_origin(source, &hop.url)
                    }))
        }) {
            return Err(HttpError::new(
                "Request redirect violates same-origin mode",
                ErrorCode::Network,
            ));
        }
        let mut res = cached_request(opts, &hop, &cl).await?;
        let final_url = final_url(&res);
        if let (Some(effects), Some(observer)) = (
            res.extensions().get::<NetworkResponseEffects>(),
            opts.response_side_effects,
        ) {
            let context = opts.context.clone().unwrap_or_default();
            if context.sends_credentials(&effects.url) {
                observer(&context, &effects.url, &effects.headers);
            }
        }
        if let Some(allowed) = opts.response_policy {
            let context = opts.context.clone().unwrap_or_default();
            let headers = res
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_string(), value.to_string()))
                })
                .collect::<Vec<_>>();
            if !allowed(&context, &hop.url, &headers) {
                return Err(HttpError::new(
                    "Response blocked by native document policy",
                    ErrorCode::Network,
                ));
            }
        }
        if res.status().as_u16() == 204 {
            if let Some(pending) = res.extensions_mut().remove::<PendingStore>() {
                pending.cache.store(pending.entry);
            }
        }
        match redirect_location(&res) {
            Some(_) if opts.redirect == Some(RedirectMode::Error) => {
                return Err(HttpError::new(
                    "Request redirect mode is error",
                    ErrorCode::Network,
                ))
            }
            Some(_) if opts.redirect == Some(RedirectMode::Manual) => {
                return Ok((res, final_url, false))
            }
            Some(loc) if redirects < max_redirects => {
                let next = advance(&hop, &res, &loc)?;
                if streaming_upload && next.method != "GET" && res.status().as_u16() != 303 {
                    return Err(HttpError::new(
                        "Cannot replay a streaming request body",
                        ErrorCode::Network,
                    ));
                }

                if !crate::request_context::same_origin(&hop.url, &next.url) {
                    opts.headers
                        .retain(|name, _| !name.eq_ignore_ascii_case("authorization"));
                }
                if hop.method != next.method {
                    opts.headers.retain(|name, _| {
                        !matches!(
                            name.to_ascii_lowercase().as_str(),
                            "content-encoding"
                                | "content-language"
                                | "content-location"
                                | "content-type"
                                | "content-length"
                        )
                    });
                }
                if let Some(context) = opts.context.as_mut() {
                    if let Some(value) = list_header(&res, "referrer-policy") {
                        if let Some(policy) =
                            crate::referrer_policy::ReferrerPolicy::from_header(&value)
                        {
                            context.referrer_policy = policy;
                        }
                    }
                    context.follow_redirect(&hop.url, &next.url);
                }
                if let Some(mut pending) = res.extensions_mut().remove::<PendingStore>() {
                    pending.entry.body = read_capped(res, DEFAULT_MAX_BYTES).await?.to_vec();
                    pending.cache.store(pending.entry);
                }
                hop = next;
            }
            Some(_) if opts.redirect == Some(RedirectMode::Follow) => {
                return Err(HttpError::new(
                    "Request redirect limit exceeded",
                    ErrorCode::Network,
                ))
            }
            _ => return Ok((res, final_url, redirects > 0)),
        }
    }
    unreachable!("redirect loop always returns at the cap")
}

fn advance(hop: &Hop, res: &http::Response, loc: &str) -> Result<Hop, HttpError> {
    let status = res.status().as_u16();
    let method = next_method(status, &hop.method);
    let current = url::Url::parse(&hop.url).map_err(|error| err(error, ErrorCode::Network))?;
    let mut next = current
        .join(loc)
        .map_err(|error| err(error, ErrorCode::Network))?;
    if !loc.contains('#') {
        next.set_fragment(current.fragment());
    }
    let body = if method == hop.method {
        hop.body.clone()
    } else {
        None
    };
    Ok(Hop {
        url: next.to_string(),
        body_bytes: if method == hop.method {
            hop.body_bytes.clone()
        } else {
            None
        },
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
    let headers = build_headers(url, &method, opts);
    let res = send(
        &cl,
        &method,
        url,
        &headers,
        &opts.body,
        &opts.body_bytes,
        None,
    )
    .await?;
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
        // Negotiated hints and browser context must be re-derived for every
        // redirect origin; a backend's automatic follow cannot do that.
        None if opts.upload_body.is_some()
            || opts.redirect.is_some()
            || opts.http_cache.is_some()
            || opts.client_hints.is_some()
            || opts.context.is_some()
            || opts.request_policy.is_some()
            || opts.response_policy.is_some()
            || opts.response_side_effects.is_some()
            || opts.before_request.is_some() =>
        {
            follow_manually(&mut opts, url, 20, max_bytes).await
        }
        None => follow_auto(&mut opts, url, max_bytes).await,
    }
}

/// Fetch response headers and retain the real reqwest/wreq stream for pull reads.
/// Redirect, CORS/embedder, cookie and hint policies use the buffered path's exact
/// manual-hop implementation; no second transport or unchecked redirect path exists.
pub async fn fetch_stream(
    url: &str,
    mut opts: FetchOptions<'_>,
) -> Result<StreamingResult, HttpError> {
    let max_bytes = opts.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    let redirects = opts.max_redirects.unwrap_or(20);
    let (res, final_url, redirected) = follow_manually_response(&mut opts, url, redirects).await?;
    gate_html_type(&opts, &res)?;
    let status = res.status().as_u16();
    let status_text = status_text(&res);
    let content_type = header_value(&res, "content-type");
    let headers = res
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect();
    let trace_id = res
        .extensions()
        .get::<crate::http_trace::RequestId>()
        .map(|id| id.0)
        .unwrap_or(0);
    let cache_store = res.extensions().get::<PendingStore>().cloned();
    let trace_prefix = res
        .extensions()
        .get::<TracePrefix>()
        .map_or(0, |prefix| prefix.0);
    let stream = res
        .bytes_stream()
        .map(|chunk| chunk.map_err(|error| HttpError::new(error.to_string(), ErrorCode::Network)));
    let body = ResponseBody {
        stream: Some(Box::pin(stream)),
        received: 0,
        max_bytes,
        trace_id,
        url: final_url.clone(),
        status,
        content_type: content_type.clone(),
        trace_body: (trace_id != 0).then(Vec::new),
        trace_prefix,
        trace_skip_prefix: trace_prefix,
        cache_store,
        background: None,
    };
    Ok(StreamingResult {
        body,
        headers,
        final_url,
        status,
        status_text,
        redirected,
        content_type,
    })
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
    if opts.redirect.is_some()
        || opts.max_redirects.is_some()
        || opts.http_cache.is_some()
        || opts.client_hints.is_some()
        || opts.context.is_some()
        || opts.request_policy.is_some()
        || opts.response_policy.is_some()
        || opts.response_side_effects.is_some()
        || opts.before_request.is_some()
    {
        let redirects = opts.max_redirects.unwrap_or(20);
        let (res, final_url, _) = follow_manually_response(&mut opts, url, redirects).await?;
        let status = res.status().as_u16();
        let cache_store = res.extensions().get::<PendingStore>().cloned();
        let bytes = read_capped(res, max_bytes).await?;
        if let Some(mut pending) = cache_store {
            pending.entry.body = bytes[pending.skip_prefix.min(bytes.len())..].to_vec();
            pending.cache.store(pending.entry);
        }
        return Ok((final_url, bytes, status));
    }
    let cl = match opts.client {
        Some(c) => c.clone(),
        None => {
            client(http::redirect::Policy::limited(20)).map_err(|e| err(e, ErrorCode::Network))?
        }
    };
    let method = opts.method.clone().unwrap_or_else(|| "GET".to_string());
    let headers = build_headers(url, &method, &opts);
    let res = send(
        &cl,
        &method,
        url,
        &headers,
        &opts.body,
        &opts.body_bytes,
        None,
    )
    .await?;
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
        let h = build_headers("http://localhost/", "GET", &opts);
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
        let h = build_headers("http://example.com/", "GET", &opts);
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
    async fn streaming_headers_settle_before_body_and_pull_preserves_binary_chunks() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        let (last_tx, last_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nSet-Cookie: sid=stream; HttpOnly; Path=/\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
            socket.flush().await.unwrap();
            first_rx.await.unwrap();
            socket.write_all(b"2\r\n\x00\xff\r\n").await.unwrap();
            socket.flush().await.unwrap();
            last_rx.await.unwrap();
            socket
                .write_all(b"2\r\n\xc3\xa9\r\n0\r\n\r\n")
                .await
                .unwrap();
        });
        let target = format!("http://{address}/binary");
        let mut jar = CookieJar::new();
        let mut response = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            fetch_stream(
                &target,
                FetchOptions {
                    allow_non_html: true,
                    jar: Some(&mut jar),
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(jar.cookie_header(&target, 0.0), "sid=stream");
        assert!(jar.document_cookie(&target, 0.0).is_empty());
        first_tx.send(()).unwrap();
        assert_eq!(
            response.body.next_chunk().await.unwrap(),
            Some(vec![0, 255])
        );
        last_tx.send(()).unwrap();
        let mut remaining = Vec::new();
        while let Some(chunk) = response.body.next_chunk().await.unwrap() {
            remaining.extend(chunk)
        }
        assert_eq!(remaining, vec![195, 169]);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn streaming_cancel_closes_unread_transport_and_limits_fail_at_read_time() {
        for limit in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let (send_tx, send_rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 4096];
                assert!(socket.read(&mut bytes).await.unwrap() > 0);
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
                socket.flush().await.unwrap();
                send_rx.await.unwrap();
                if limit {
                    socket.write_all(b"4\r\nbody\r\n").await.unwrap();
                    socket.flush().await.unwrap()
                }
                tokio::time::timeout(std::time::Duration::from_secs(3), socket.read(&mut bytes))
                    .await
                    .unwrap()
                    .unwrap()
            });
            let mut response = fetch_stream(
                &format!("http://{address}/pending"),
                FetchOptions {
                    allow_non_html: true,
                    max_bytes: Some(3),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            send_tx.send(()).unwrap();
            if limit {
                assert_eq!(
                    response.body.next_chunk().await.unwrap_err().code,
                    ErrorCode::BodyTooLarge
                )
            } else {
                response.body.cancel()
            }
            assert!(response.body.next_chunk().await.unwrap().is_none());
            assert_eq!(task.await.unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn streaming_redirects_keep_cookie_and_native_request_response_policy_guards() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let target = format!("http://{address}/start");
        let task = tokio::spawn(async move {
            for index in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 4096];
                let length = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..length]);
                if index == 0 {
                    socket
                        .write_all(&http(
                            "302 Found",
                            &[("Location", "/finish"), ("Set-Cookie", "hop=yes; Path=/")],
                            b"",
                        ))
                        .await
                        .unwrap();
                } else {
                    assert!(request.to_ascii_lowercase().contains("cookie: hop=yes"));
                    socket
                        .write_all(&http("200 OK", &[("Content-Type", "text/plain")], b"ok"))
                        .await
                        .unwrap();
                }
            }
        });
        let policy = |url: &str, _| url.ends_with("/start") || url.ends_with("/finish");
        let responses = |_: &RequestContext, url: &str, _: &[(String, String)]| {
            url.ends_with("/start") || url.ends_with("/finish")
        };
        let mut jar = CookieJar::new();
        let mut response = fetch_stream(
            &target,
            FetchOptions {
                allow_non_html: true,
                jar: Some(&mut jar),
                request_policy: Some(&policy),
                response_policy: Some(&responses),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(response.redirected);
        assert!(response.final_url.ends_with("/finish"));
        assert_eq!(
            response.body.next_chunk().await.unwrap(),
            Some(b"ok".to_vec())
        );
        assert!(response.body.next_chunk().await.unwrap().is_none());
        task.await.unwrap();
        let refused = |_: &str, _| false;
        let error = fetch_stream(
            "http://127.0.0.1:1/refused",
            FetchOptions {
                request_policy: Some(&refused),
                ..Default::default()
            },
        )
        .await
        .err()
        .unwrap();
        assert!(error.message.contains("native document policy"));
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
    async fn native_response_policy_denies_a_redirect_before_following_it() {
        let p = spawn().await;
        let seen = std::sync::Mutex::new(Vec::new());
        let request = |target: &str, _| {
            seen.lock().unwrap().push(target.to_string());
            true
        };
        let response = |_: &RequestContext, _: &str, headers: &[(String, String)]| {
            !headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("location"))
        };
        let result = fetch_html(
            &url(p, "/r"),
            FetchOptions {
                request_policy: Some(&request),
                response_policy: Some(&response),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            result.unwrap_err().message,
            "Response blocked by native document policy"
        );
        assert_eq!(*seen.lock().unwrap(), vec![url(p, "/r")]);
    }

    #[tokio::test]
    async fn native_policy_checks_each_redirect_target_for_text_and_binary() {
        let p = spawn().await;
        let seen = std::sync::Mutex::new(Vec::new());
        let policy = |target: &str, redirected: bool| {
            seen.lock().unwrap().push((target.to_string(), redirected));
            !target.ends_with("/dest")
        };
        let result = fetch_html(
            &url(p, "/r"),
            FetchOptions {
                request_policy: Some(&policy),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            result.unwrap_err().message,
            "Request blocked by native document policy"
        );
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(url(p, "/r"), false), (url(p, "/dest"), true)]
        );
        seen.lock().unwrap().clear();
        let result = fetch_bytes(
            &url(p, "/r"),
            FetchOptions {
                request_policy: Some(&policy),
                ..Default::default()
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(url(p, "/r"), false), (url(p, "/dest"), true)]
        );
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

#[cfg(test)]
mod response_chunk_tests {
    use super::*;

    fn body(chunks: Vec<Result<bytes::Bytes, HttpError>>) -> ResponseBody {
        ResponseBody {
            stream: Some(Box::pin(futures_util::stream::iter(chunks))),
            received: 0,
            max_bytes: 3,
            trace_id: 0,
            url: "https://transport-frames.test/".into(),
            status: 200,
            content_type: "text/plain".into(),
            trace_body: None,
            trace_prefix: 0,
            trace_skip_prefix: 0,
            cache_store: None,
            background: None,
        }
    }

    #[tokio::test]
    async fn empty_transport_frames_do_not_reach_the_body_consumer() {
        let mut response = body(vec![
            Ok(bytes::Bytes::new()),
            Ok(bytes::Bytes::from_static(b"a")),
            Ok(bytes::Bytes::new()),
            Ok(bytes::Bytes::from_static(b"bc")),
            Ok(bytes::Bytes::new()),
        ]);
        assert_eq!(response.next_chunk().await.unwrap(), Some(b"a".to_vec()));
        assert_eq!(response.next_chunk().await.unwrap(), Some(b"bc".to_vec()));
        assert_eq!(response.next_chunk().await.unwrap(), None);
        assert_eq!(response.received, 3);
    }

    #[tokio::test]
    async fn empty_only_transport_closes_without_a_zero_length_byte_chunk() {
        let mut response = body(vec![Ok(bytes::Bytes::new()), Ok(bytes::Bytes::new())]);
        assert_eq!(response.next_chunk().await.unwrap(), None);
        assert_eq!(response.received, 0);
    }

    #[tokio::test]
    async fn transport_error_after_empty_frame_is_preserved_and_releases_stream() {
        let mut response = body(vec![
            Ok(bytes::Bytes::new()),
            Err(HttpError::new("authored reset", ErrorCode::Network)),
        ]);
        let error = response.next_chunk().await.unwrap_err();
        assert_eq!(error.message, "authored reset");
        assert!(matches!(error.code, ErrorCode::Network));
        assert!(response.stream.is_none());
        assert_eq!(response.next_chunk().await.unwrap(), None);
    }
}
