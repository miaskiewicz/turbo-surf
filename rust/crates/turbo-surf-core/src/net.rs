//! Networking (port of `src/net.mjs`, SPEC §8): `fetch_html` over reqwest
//! (redirects + gzip/br/deflate for free) plus the hardening the spec calls for:
//! charset sniffing, max body size, content-type gate, and an optional CookieJar
//! round-trip. The pure helpers below are unit-tested offline; `fetch_html`
//! itself is the live-IO seam (covered by the integration suite / harness).

use crate::cache::ResponseCache;
use crate::compression_dictionary::{DictionaryStore, PendingDictionary, SelectedDictionary};
use crate::cookies::CookieJar;
use crate::http_backend as http; // reqwest (rustls) or wreq (BoringSSL); see lib.rs
use crate::http_cache::{CacheMode, Cached, HttpCache, PendingStore, TracePrefix};
use crate::http_cache_ranges::ByteRange;
use crate::request_context::{ClientHints, RequestContext};
use bytes::BytesMut;
use futures_util::StreamExt;
use std::collections::BTreeMap;

const DEFAULT_MAX_BYTES: usize = 8 * 1024 * 1024; // 8 MiB

/// Chrome target of the measured TLS/H2/header profile: the default registry
/// release, whose headers are measured independently of the upstream TLS/H2
/// recipe it reuses (reported separately by [`transport_base_chrome_major`]).
/// Rustls has no Chrome transport profile.
pub fn transport_chrome_major() -> Option<u16> {
    #[cfg(feature = "impersonate")]
    return Some(crate::browser::DEFAULT_MAJOR);
    #[cfg(not(feature = "impersonate"))]
    None
}

/// Underlying upstream recipe, distinct from the measured target profile.
pub fn transport_base_chrome_major() -> Option<u16> {
    #[cfg(feature = "impersonate")]
    return Some(crate::browser::default_release().transport_base_major);
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

#[derive(Debug, Clone)]
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
/// A transport terminal result, independent of guest stream consumption.
pub type ResponseCompletionObserver = std::sync::Arc<dyn Fn(Result<(), HttpError>) + Send + Sync>;

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
    dictionary_store: Option<PendingDictionary>,
    background: Option<std::sync::Arc<BackgroundBody>>,
}
struct BackgroundBody {
    cache_fill: bool,
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
        owner: tokio::sync::watch::Receiver<bool>,
        observer: Option<ResponseChunkObserver>,
    ) -> Self {
        self.background_observed_impl(owner, observer, None, false)
    }
    /// Download and observe transport completion independently of guest body pulls.
    pub fn background_observed(
        self,
        owner: tokio::sync::watch::Receiver<bool>,
        observer: ResponseChunkObserver,
        completion: ResponseCompletionObserver,
    ) -> Self {
        self.background_observed_impl(owner, Some(observer), Some(completion), true)
    }
    fn background_observed_impl(
        self,
        mut owner: tokio::sync::watch::Receiver<bool>,
        observer: Option<ResponseChunkObserver>,
        completion: Option<ResponseCompletionObserver>,
        force: bool,
    ) -> Self {
        if !force && self.cache_store.is_none() {
            return self;
        }
        let (cancel, mut canceled) = tokio::sync::watch::channel(false);
        let background = std::sync::Arc::new(BackgroundBody {
            cache_fill: self.cache_store.is_some(),
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
            dictionary_store: None,
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
            if let Some(completion) = completion {
                completion(error.clone().map_or(Ok(()), Err));
            }
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
        if !background.cache_fill {
            return false;
        }
        background.detach();
        true
    }
    /// An external response observer owns completion independently of guest GC.
    pub fn detach_observed_consumer(&mut self) -> bool {
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
                    if let Some(pending) = self.dictionary_store.take() {
                        pending.commit();
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
        if self
            .dictionary_store
            .as_mut()
            .is_some_and(|pending| !pending.append(&chunk))
        {
            self.dictionary_store = None;
        }
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
        self.dictionary_store = None;
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
struct HeadResponse;

#[derive(Clone)]
struct NetworkResponseEffects {
    url: String,
    headers: Vec<(String, String)>,
}

/// Metadata observed from an actual admitted HTTP response before its body is read.
#[derive(Clone, Debug)]
pub struct ResponseObservation {
    pub url: String,
    pub method: String,
    pub request_headers: BTreeMap<String, String>,
    pub request_body: Option<Vec<u8>>,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
}
pub type ResponseObserver<'a> = dyn Fn(ResponseObservation) + Send + Sync + 'a;

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
    pub http_credentials: Option<&'a crate::http_auth::Credentials>,
    pub headers: BTreeMap<String, String>,
    /// Browser metadata for this request. Unspecified requests are top-level
    /// navigations; script/fetch/image callers should identify their context.
    pub context: Option<RequestContext>,
    /// A native embedder policy checked before contacting every redirect target.
    pub request_policy: Option<&'a RequestPolicy<'a>>,
    /// Actual admitted response metadata, including intermediate redirect hops.
    pub response_observer: Option<&'a ResponseObserver<'a>>,
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
    /// Explicit override; otherwise dictionaries share the native HTTP cache's session.
    pub dictionary_store: Option<&'a DictionaryStore>,
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
    // Chrome sends no client hints at all on a CORS preflight (measured by the
    // identity battery), not even the low-entropy sec-ch-ua trio.
    let preflight = method.eq_ignore_ascii_case("OPTIONS")
        && opts
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("access-control-request-method"));
    if !preflight {
        h.extend(
            opts.client_hints
                .unwrap_or(&no_opt_ins)
                .headers(url, context, profile),
        );
    }
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
// The measured TLS/H2 capabilities reuse the registry's upstream recipe; the
// identity and navigation header order are pinned to fingerprint::default_profile(). This
// is an emulation, not proof of current Chrome wire parity. Validate the deployed
// build with the fingerprint tests, especially after a Chrome/backend upgrade.
// Per-call profile overrides are not applied on this backend.
fn emulate(builder: http::ClientBuilder) -> http::ClientBuilder {
    #[cfg(feature = "impersonate")]
    let builder = apply_emulation(builder, chrome_emulation());
    builder
}

#[cfg(feature = "impersonate")]
fn chrome_emulation() -> wreq::Emulation {
    {
        use wreq::IntoEmulation;
        let profile = crate::fingerprint::default_profile();
        // Resolve the upstream wreq-util Chrome profile to a concrete `Emulation` so its
        // header set, header ORDER and TLS options can be tuned to match a real
        // Chrome *cold* top-level navigation before it's applied.
        let mut emulation = wreq_util::Emulation::builder()
            .profile(transport_recipe(
                crate::browser::default_release().transport_base_major,
            ))
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
        emulation
    }
}

#[cfg(feature = "impersonate")]
fn apply_emulation(
    builder: http::ClientBuilder,
    emulation: wreq::Emulation,
) -> http::ClientBuilder {
    builder
        .emulation(emulation)
        // Override AFTER emulation (wreq applies the profile immediately, so
        // later fine-tuning wins) to use the configured full patch version
        // from the browser registry.
        .user_agent(crate::fingerprint::default_profile().user_agent.as_str())
}

/// The transport for a `wss:` upgrade (#179): the https emulation with the TLS
/// change Chrome's WebSocket stack makes for an HTTP/1.1 upgrade. ALPN offers only
/// `http/1.1`, and Chrome configures ALPS only for offered protocols, so no ALPS
/// either. No redirects (Chrome fails a redirected handshake) and no idle pooling
/// (an upgraded or refused connection is never lent to another request). The rustls
/// backend has no Chrome transport and dials `wss:` itself (websocket.rs).
#[cfg(feature = "impersonate")]
pub(crate) fn websocket_client_builder(
    anchor: Option<&[u8]>,
) -> Result<http::ClientBuilder, HttpError> {
    let builder = {
        let mut emulation = chrome_emulation();
        if let Some(tls) = emulation.tls_options.as_mut() {
            tls.alpn_protocols = Some(std::borrow::Cow::Borrowed(&[
                http::tls::AlpnProtocol::HTTP1,
            ]));
            tls.alps_protocols = None;
        }
        apply_emulation(http::Client::builder(), emulation)
    };
    let builder = builder
        .redirect(http::redirect::Policy::none())
        .pool_max_idle_per_host(0);
    match anchor {
        Some(der) => with_trust(builder, der),
        None => Ok(builder),
    }
}

// The upstream wreq-util recipe a registry release names. Only recipes whose
// ClientHello and H2 frames were measured equal to a Chrome release are listed;
// adding one is a transport change (see harness/detection/CHROME-REVERSION.md).
#[cfg(feature = "impersonate")]
fn transport_recipe(base_major: u16) -> wreq_util::Profile {
    match base_major {
        154 => wreq_util::Profile::Chrome154,
        other => panic!("browser_releases.json names unsupported transport recipe Chrome{other}"),
    }
}

// Chrome's measured wire order for this request's class (navigation, no-cors,
// CORS, preflight, hinted …), cached per class. `None` keeps the client-level
// navigation order, which is all releases measured before the battery recorded.
#[cfg(feature = "impersonate")]
fn request_order(
    method: &str,
    headers: &BTreeMap<String, String>,
    restart: bool,
) -> Option<http::header::OrigHeaderMap> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, http::header::OrigHeaderMap>>> = OnceLock::new();
    let mut class = crate::browser::request_class(method, |name| headers.get(name).cloned());
    let release = crate::browser::default_release();
    // A Critical-CH restart has its own measured order; fall back to the class's.
    if restart && release.request_order(&format!("{class}-restart")).is_some() {
        class.push_str("-restart");
    }
    let order = release.request_order(&class)?;
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    Some(
        cache
            .entry(class)
            .or_insert_with(|| {
                let mut map = http::header::OrigHeaderMap::new();
                for name in order {
                    map.insert(*name);
                }
                map
            })
            .clone(),
    )
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
    with_trust(
        client_builder().redirect(http::redirect::Policy::none()),
        der,
    )
}
fn with_trust(builder: http::ClientBuilder, der: &[u8]) -> Result<http::ClientBuilder, HttpError> {
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

/// One outgoing hop through the transport seam.
struct Outgoing<'a> {
    method: &'a str,
    url: &'a str,
    headers: &'a BTreeMap<String, String>,
    body: &'a Option<String>,
    body_bytes: &'a Option<Vec<u8>>,
    upload: Option<crate::upload_body::UploadBody>,
    /// A Critical-CH restart, which has its own measured header order.
    restart: bool,
}

// One transport seam for every hop.
async fn send(cl: &http::Client, hop: Outgoing<'_>) -> Result<http::Response, HttpError> {
    let Outgoing {
        method,
        url,
        headers,
        body,
        body_bytes,
        upload,
        restart,
    } = hop;
    let m = http::Method::from_bytes(method.as_bytes()).map_err(|e| err(e, ErrorCode::Network))?;
    // Without impersonation a streaming upload is forced onto HTTP/2 below.
    let force_http2 = upload.is_some() && cfg!(not(feature = "impersonate"));
    let wire = wire_headers(url, headers, force_http2);
    let mut req = apply_headers(cl.request(m, url), &wire);
    #[cfg(feature = "impersonate")]
    if let Some(order) = request_order(method, &wire, restart) {
        req = req.orig_headers(order);
    }
    #[cfg(not(feature = "impersonate"))]
    let _ = restart;
    // Fetch already chose a Range request's Accept-Encoding (identity, plus
    // dcb/dcz with a dictionary). wreq's decoder would overwrite it with bare
    // identity, so it owns no content coding for this request.
    #[cfg(feature = "impersonate")]
    if wire.contains_key("range") {
        req = req.gzip(false).brotli(false).deflate(false).zstd(false);
    }
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
    if let Ok(response) = &result {
        remember_protocol(url, response.version());
    }
    if method == "HEAD" {
        if let Ok(response) = &mut result {
            response.extensions_mut().insert(HeadResponse);
        }
    }
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
    result.map_err(|e| transport_error(&e, url))
}

fn transport_error(error: &(dyn std::error::Error + 'static), url: &str) -> HttpError {
    let mut message = error.to_string();
    let mut cause = error.source();
    // Transport Display implementations may include only their first wrapper.
    for _ in 0..16 {
        let Some(current) = cause else { break };
        let detail = current.to_string();
        if !message.ends_with(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        cause = current.source();
    }
    err(format!("{message} (url: {url})"), ErrorCode::Network)
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

// The protocol each origin's last response came over, like Chrome's
// HttpServerProperties. Request headers are built before ALPN picks the
// protocol, so only a known origin can drop its other protocol's headers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OriginProtocol {
    Http1,
    Http2,
}

fn origin_protocols() -> &'static std::sync::Mutex<std::collections::HashMap<String, OriginProtocol>>
{
    static ORIGINS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, OriginProtocol>>,
    > = std::sync::OnceLock::new();
    ORIGINS.get_or_init(Default::default)
}

const ORIGIN_PROTOCOL_LIMIT: usize = 4096;

fn remember_protocol(url: &str, version: http::Version) {
    let Some(origin) = crate::request_context::origin(url) else {
        return;
    };
    let protocol = if version == http::Version::HTTP_2 || version == http::Version::HTTP_3 {
        OriginProtocol::Http2
    } else {
        OriginProtocol::Http1
    };
    let mut origins = origin_protocols().lock().unwrap_or_else(|e| e.into_inner());
    if origins.len() >= ORIGIN_PROTOCOL_LIMIT && !origins.contains_key(&origin) {
        origins.clear();
    }
    origins.insert(origin, protocol);
}

fn known_protocol(url: &str) -> Option<OriginProtocol> {
    let origin = crate::request_context::origin(url)?;
    origin_protocols()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&origin)
        .copied()
}

// Transport-owned headers Chrome adds per protocol. `Connection: keep-alive` is
// HTTP/1.1 only; `Priority` is HTTP/2+ only. An origin known to speak HTTP/2 (or
// a request forced onto HTTP/2) loses `Connection` here, so the HTTP/2 encoder
// never has to strip it (and warn). An unknown origin keeps both: its first
// request may still negotiate HTTP/1.1.
fn wire_headers(
    url: &str,
    headers: &BTreeMap<String, String>,
    force_http2: bool,
) -> BTreeMap<String, String> {
    let mut wire = headers.clone();
    let known = if force_http2 {
        Some(OriginProtocol::Http2)
    } else {
        known_protocol(url)
    };
    match known {
        Some(OriginProtocol::Http2) => {
            wire.remove("connection");
        }
        Some(OriginProtocol::Http1) => {
            wire.remove("priority");
            wire.entry("connection".into())
                .or_insert_with(|| "keep-alive".into());
        }
        None => {
            wire.entry("connection".into())
                .or_insert_with(|| "keep-alive".into());
        }
    }
    wire
}

fn check_content_length(res: &http::Response, max_bytes: usize) -> Result<(), HttpError> {
    if res.extensions().get::<HeadResponse>().is_some() {
        return Ok(());
    }
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

fn dictionary_encoding(res: &http::Response) -> Result<Option<String>, HttpError> {
    if res.extensions().get::<Cached>().is_some()
        || res.extensions().get::<HeadResponse>().is_some()
    {
        return Ok(None);
    }
    let encoding = list_header(res, "content-encoding")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !encoding
        .split(',')
        .any(|part| matches!(part.trim(), "dcb" | "dcz"))
    {
        return Ok(None);
    }
    if !matches!(encoding.as_str(), "dcb" | "dcz") {
        return Err(HttpError::new(
            "Unsupported dictionary encoding chain",
            ErrorCode::Network,
        ));
    }
    Ok(Some(encoding))
}
async fn read_capped(res: http::Response, max_bytes: usize) -> Result<Vec<u8>, HttpError> {
    let decoder = match dictionary_encoding(&res)? {
        Some(encoding) => {
            let dictionary = res
                .extensions()
                .get::<SelectedDictionary>()
                .cloned()
                .ok_or_else(|| {
                    HttpError::new(
                        "Dictionary response without an advertised dictionary",
                        ErrorCode::Network,
                    )
                })?;
            Some((encoding, dictionary))
        }
        None => None,
    };
    let mut pending = res.extensions().get::<PendingDictionary>().cloned();
    let encoded_maximum = if decoder.is_some() {
        crate::compression_dictionary::decode::MAX_ENCODED_BYTES
    } else {
        max_bytes
    };
    check_content_length(&res, encoded_maximum)?;
    // Bound encoded buffering as well as CPU workers; canceled waiters retain only transport backpressure.
    let decoder = match decoder {
        Some((encoding, dictionary)) => Some((
            encoding,
            dictionary,
            crate::compression_dictionary::decode::reserve().await,
        )),
        None => None,
    };
    let mut buf = BytesMut::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| HttpError::new(e.to_string(), ErrorCode::Network))?;
        if chunk.len() > encoded_maximum.saturating_sub(buf.len()) {
            return Err(HttpError::new(
                format!("body exceeds maxBytes (> {encoded_maximum})"),
                ErrorCode::BodyTooLarge,
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    let body = match decoder {
        Some((encoding, dictionary, permit)) => crate::compression_dictionary::decode::decode(
            encoding,
            buf.to_vec(),
            dictionary,
            max_bytes,
            permit,
        )
        .await
        .map_err(|error| {
            let code = if matches!(
                error,
                crate::compression_dictionary::decode::DecodeError::Size
            ) {
                ErrorCode::BodyTooLarge
            } else {
                ErrorCode::Network
            };
            HttpError::new(format!("Dictionary response decode failed: {error}"), code)
        })?,
        None => buf.to_vec(),
    };
    if let Some(mut registration) = pending.take() {
        if registration.append(&body) {
            registration.commit();
        }
    }
    Ok(body)
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
    if let Some(store) = opts.dictionary_store {
        if crate::request_context::trustworthy(final_url)
            && list_header(res, "clear-site-data")
                .unwrap_or_default()
                .split(',')
                .any(|part| matches!(part.trim(), "\"cache\"" | "\"*\""))
        {
            store.clear_origin(final_url);
        }
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
                // invariant: selected_range(None) is always Some (the full body).
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

// Chrome restarts a navigation once when the response's Critical-CH names a
// hint that its Accept-CH just enabled and the request did not carry.
async fn cached_request(
    opts: &mut FetchOptions<'_>,
    hop: &Hop,
    client: &http::Client,
) -> Result<http::Response, HttpError> {
    let navigation = opts
        .context
        .as_ref()
        .is_none_or(|context| context.mode == crate::request_context::RequestMode::Navigate);
    let replayable = navigation && opts.upload_body.is_none() && opts.client_hints.is_some();
    let sent = replayable.then(|| build_headers(&hop.url, &hop.method, opts));
    let response = cached_request_once(opts, hop, client, false).await?;
    let Some(sent) = sent else {
        return Ok(response);
    };
    let critical = list_header(&response, "critical-ch").unwrap_or_default();
    let now = build_headers(&hop.url, &hop.method, opts);
    let restart = critical.split(',').any(|hint| {
        let hint = hint.trim().to_ascii_lowercase();
        !hint.is_empty() && now.contains_key(&hint) && !sent.contains_key(&hint)
    });
    if !restart {
        return Ok(response);
    }
    drop(response);
    cached_request_once(opts, hop, client, true).await
}

/// Arguments to [`send_and_revalidate`] — the "no reusable cached response"
/// path through [`cached_request_once`].
struct SendAndRevalidate<'a, 'b> {
    opts: &'a mut FetchOptions<'b>,
    hop: &'a Hop,
    client: &'a http::Client,
    restart: bool,
    headers: &'a mut BTreeMap<String, String>,
    mode: CacheMode,
    range_header: Option<&'a str>,
    candidate: Option<(crate::http_cache::Entry, bool)>,
    fill: Option<RangeFill>,
    generation: u64,
    cache: Option<&'a HttpCache>,
}

/// Merge revalidation / range / cache-control request headers from the
/// candidate cached entry, then send the request. Split from
/// [`cached_request_once`] to keep its cyclomatic complexity under the gate.
async fn send_and_revalidate<'a, 'b>(
    args: SendAndRevalidate<'a, 'b>,
) -> Result<http::Response, HttpError> {
    let SendAndRevalidate {
        opts,
        hop,
        client,
        restart,
        headers,
        mode,
        range_header,
        candidate,
        fill,
        generation,
        cache,
    } = args;
    if mode == CacheMode::OnlyIfCached {
        return Err(HttpError::new("HTTP cache miss", ErrorCode::Network));
    }
    apply_revalidation_headers(headers, &fill, range_header, candidate.as_ref());
    apply_cache_control_mode(headers, mode);
    let mut response = send(
        client,
        Outgoing {
            method: &hop.method,
            url: &hop.url,
            headers,
            body: &hop.body,
            body_bytes: &hop.body_bytes,
            upload: opts.upload_body.take(),
            restart,
        },
    )
    .await?;
    if opts.response_side_effects.is_some() {
        let effects = NetworkResponseEffects {
            url: final_url(&response),
            headers: response_headers_pairs(&response),
        };
        response.extensions_mut().insert(effects);
    }
    let raw_effects = response
        .extensions()
        .get::<NetworkResponseEffects>()
        .cloned();
    ingest_set_cookie(opts, &response, &hop.url);
    if let Some(fill) = fill.filter(|_| response.status().as_u16() == 206) {
        return finish_range_response(opts, hop, headers, response, fill, generation);
    }
    if response.status().as_u16() == 304 {
        return finish_304_revalidation(
            response,
            candidate,
            headers,
            range_header,
            raw_effects,
            hop.method.as_str(),
            cache,
        );
    }
    Ok(response)
}

/// Install range / if-range / cached validators on the request headers for a
/// revalidation or partial-fill hop.
fn apply_revalidation_headers(
    headers: &mut BTreeMap<String, String>,
    fill: &Option<RangeFill>,
    range_header: Option<&str>,
    candidate: Option<&(crate::http_cache::Entry, bool)>,
) {
    if let Some(fill) = fill {
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
        if let Some((entry, _)) = candidate {
            entry.validators(headers);
        }
    }
}

/// Seed `cache-control` / `pragma` request headers for Reload / NoStore /
/// NoCache modes (each left untouched when the caller already set the field).
fn apply_cache_control_mode(headers: &mut BTreeMap<String, String>, mode: CacheMode) {
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
}

/// Convert a response's header map into the `(name, value)` owned-pairs form
/// used by cache entries and response-effects records.
fn response_headers_pairs(response: &http::Response) -> Vec<(String, String)> {
    response
        .headers()
        .iter()
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (key.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

/// Merge the 304 response headers into the cached entry, store the refreshed
/// entry, and produce the client-visible response (honouring HEAD + effects).
fn finish_304_revalidation(
    response: http::Response,
    candidate: Option<(crate::http_cache::Entry, bool)>,
    headers: &BTreeMap<String, String>,
    range_header: Option<&str>,
    raw_effects: Option<NetworkResponseEffects>,
    method: &str,
    cache: Option<&HttpCache>,
) -> Result<http::Response, HttpError> {
    let Some((mut entry, _)) = candidate else {
        return Ok(response);
    };
    let response_headers = response_headers_pairs(&response);
    entry.merge(&response_headers, headers);
    let mut prepared = entry.range_response(range_header).ok_or_else(|| {
        HttpError::new(
            "Incomplete cached response for revalidation",
            ErrorCode::Network,
        )
    })?;
    if let Some(cache) = cache {
        cache.store(entry);
    }
    if method == "HEAD" {
        prepared.body.clear();
    }
    let mut response = prepared.response();
    if let Some(effects) = raw_effects {
        response.extensions_mut().insert(effects);
    }
    Ok(response)
}

/// Fetch's conditional-request bypass: when the caller supplied any If-* header
/// on a Default-cache-mode request, drop the HTTP cache for this hop and send
/// to the network (so surf never short-circuits a caller's revalidation).
fn resolve_cache_mode(opts: &FetchOptions<'_>, headers: &BTreeMap<String, String>) -> CacheMode {
    const CONDITIONAL: &[&str] = &[
        "if-none-match",
        "if-modified-since",
        "if-unmodified-since",
        "if-match",
        "if-range",
    ];
    let conditional = CONDITIONAL.iter().any(|name| headers.contains_key(*name));
    if opts.http_cache.is_some() && opts.cache_mode == CacheMode::Default && conditional {
        CacheMode::NoStore
    } else {
        opts.cache_mode
    }
}

async fn cached_request_once(
    opts: &mut FetchOptions<'_>,
    hop: &Hop,
    client: &http::Client,
    restart: bool,
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
    // Fetch's HTTP-network-or-cache fetch: a Range request asks for identity
    // (Chrome 156 control `header-range-request` / `range-outside-match`).
    if headers.contains_key("range") {
        headers.insert("accept-encoding".into(), "identity".into());
    }
    let dictionary_store = opts
        .dictionary_store
        .or_else(|| opts.http_cache.map(HttpCache::dictionary_store))
        .cloned();
    let dictionary_generation = dictionary_store
        .as_ref()
        .map_or(0, DictionaryStore::generation);
    let dictionary_context = opts.context.clone().unwrap_or_default();
    let selected_dictionary = dictionary_store
        .as_ref()
        .and_then(|store| store.select(&opts.cache_partition, &hop.url, &dictionary_context));
    if let Some(dictionary) = &selected_dictionary {
        dictionary.negotiate(&mut headers);
    }
    let cache = opts.http_cache;
    let generation = cache.map_or(0, HttpCache::generation);
    let eligible = matches!(hop.method.as_str(), "GET" | "HEAD");
    let range_header = (hop.method == "GET")
        .then(|| headers.get("range").cloned())
        .flatten();
    let mode = resolve_cache_mode(opts, &headers);
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
        send_and_revalidate(SendAndRevalidate {
            opts,
            hop,
            client,
            restart,
            headers: &mut headers,
            mode,
            range_header: range_header.as_deref(),
            candidate,
            fill,
            generation,
            cache,
        })
        .await?
    };
    attach_post_response_extensions(
        opts,
        hop,
        &headers,
        &mut res,
        PostResponseContext {
            cache,
            mode,
            eligible,
            generation,
            selected_dictionary,
            dictionary_store,
            dictionary_generation,
            dictionary_context: &dictionary_context,
        },
    );
    Ok(res)
}

/// Inputs threaded into [`attach_post_response_extensions`] — a struct beats a
/// 10-arg function for readability and keeps the field names in sight at the
/// call site.
struct PostResponseContext<'a> {
    cache: Option<&'a HttpCache>,
    mode: CacheMode,
    eligible: bool,
    generation: u64,
    selected_dictionary: Option<SelectedDictionary>,
    dictionary_store: Option<DictionaryStore>,
    dictionary_generation: u64,
    dictionary_context: &'a RequestContext,
}

/// Invalidate the cache on unsafe writes, attach pending-store / dictionary /
/// head markers, and stash a pending compression dictionary. Split out of
/// [`cached_request_once`] to keep its complexity under the gate.
fn attach_post_response_extensions(
    opts: &FetchOptions<'_>,
    hop: &Hop,
    headers: &BTreeMap<String, String>,
    res: &mut http::Response,
    ctx: PostResponseContext<'_>,
) {
    let PostResponseContext {
        cache,
        mode,
        eligible,
        generation,
        selected_dictionary,
        dictionary_store,
        dictionary_generation,
        dictionary_context,
    } = ctx;
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
                headers,
                res,
                status_text(res),
                generation,
            )
        }) {
            res.extensions_mut().insert(pending);
        }
    }
    if let Some(dictionary) = selected_dictionary {
        res.extensions_mut().insert(dictionary);
    }
    if hop.method == "HEAD" {
        res.extensions_mut().insert(HeadResponse);
    }
    if hop.method != "HEAD" && mode != CacheMode::NoStore {
        if let Some(store) = &dictionary_store {
            attach_pending_dictionary(
                opts,
                hop,
                res,
                store,
                dictionary_generation,
                dictionary_context,
            );
        }
    }
}

/// If the response's headers match a compression-dictionary rule, remember the
/// pending-store entry so the body reader can finish it when the bytes drain.
fn attach_pending_dictionary(
    opts: &FetchOptions<'_>,
    hop: &Hop,
    res: &mut http::Response,
    store: &DictionaryStore,
    dictionary_generation: u64,
    dictionary_context: &RequestContext,
) {
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
    if let Some(pending) = PendingDictionary::new(
        store,
        dictionary_generation,
        &opts.cache_partition,
        &hop.url,
        dictionary_context,
        res.status().as_u16(),
        headers,
        res.extensions()
            .get::<crate::http_cache::CachedStoredAt>()
            .map_or_else(std::time::SystemTime::now, |time| time.0),
    ) {
        res.extensions_mut().insert(pending);
    }
}

fn challenge_authorization(
    opts: &FetchOptions<'_>,
    hop: &Hop,
    response: &http::Response,
) -> Option<String> {
    if response.status().as_u16() != 401
        || opts
            .context
            .as_ref()
            .is_some_and(|context| !context.sends_credentials(&hop.url))
    {
        return None;
    }
    let basic = response_headers_pairs(response)
        .iter()
        .any(|(name, value)| {
            name.eq_ignore_ascii_case("www-authenticate")
                && crate::http_auth::basic_challenge(value)
        });
    if !basic {
        return None;
    }
    opts.http_credentials?.authorization(&hop.url)
}

async fn request_with_auth(
    opts: &mut FetchOptions<'_>,
    hop: &Hop,
    client: &http::Client,
    streaming: bool,
) -> Result<http::Response, HttpError> {
    let response = cached_request(opts, hop, client).await?;
    notify_response_effects(opts, &response);
    check_response_policy(opts, hop, &response)?;
    observe_response(opts, hop, &response);
    let Some(authorization) = challenge_authorization(opts, hop, &response) else {
        return Ok(response);
    };
    if streaming {
        return Err(HttpError::new(
            "Cannot replay streaming body for HTTP authentication",
            ErrorCode::Network,
        ));
    }
    drop(response);
    opts.headers
        .retain(|name, _| !name.eq_ignore_ascii_case("authorization"));
    opts.headers.insert("authorization".into(), authorization);
    let previous_mode = opts.cache_mode;
    opts.cache_mode = CacheMode::Reload;
    let response = cached_request(opts, hop, client).await;
    opts.cache_mode = previous_mode;
    let response = response?;
    notify_response_effects(opts, &response);
    check_response_policy(opts, hop, &response)?;
    observe_response(opts, hop, &response);
    Ok(response)
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
    let cl = build_follow_client(opts, streaming_upload)?;
    let mut hop = Hop {
        url: start.to_string(),
        method: opts.method.clone().unwrap_or_else(|| "GET".to_string()),
        body: opts.body.clone(),
        body_bytes: opts.body_bytes.clone(),
    };
    for redirects in 0.. {
        check_request_policy(opts, &hop, redirects)?;
        check_same_origin_mode(opts, &hop)?;
        let mut res = request_with_auth(opts, &hop, &cl, streaming_upload).await?;
        let final_url = final_url(&res);
        if res.status().as_u16() == 204 {
            if let Some(pending) = res.extensions_mut().remove::<PendingStore>() {
                pending.cache.store(pending.entry);
            }
        }
        let loc = match redirect_dispatch(opts, redirects, max_redirects, &res) {
            RedirectDispatch::Return => return Ok((res, final_url, redirects > 0)),
            RedirectDispatch::ReturnAsFinal => return Ok((res, final_url, false)),
            RedirectDispatch::Error(e) => return Err(e),
            RedirectDispatch::Follow(loc) => loc,
        };
        hop = apply_redirect_hop(opts, hop, res, &loc, streaming_upload).await?;
    }
    unreachable!("redirect loop always returns at the cap")
}

/// Pick the reqwest client follow_manually_response should use: client-hints
/// aware when available, upload-body-aware when the caller is streaming.
fn build_follow_client(
    opts: &FetchOptions<'_>,
    streaming_upload: bool,
) -> Result<http::Client, HttpError> {
    match opts.client_hints {
        Some(hints) if streaming_upload => hints.upload_client(),
        Some(hints) => hints.manual_client(),
        None if streaming_upload => {
            crate::upload_body::guard(client_builder().redirect(http::redirect::Policy::none()))
                .build()
                .map_err(|e| err(e, ErrorCode::Network))
        }
        None => client(http::redirect::Policy::none()).map_err(|e| err(e, ErrorCode::Network)),
    }
}

/// Fetch's request-policy gate — refuse the hop when the native document
/// policy says so (first request plus every redirect).
fn check_request_policy(
    opts: &FetchOptions<'_>,
    hop: &Hop,
    redirects: usize,
) -> Result<(), HttpError> {
    if opts
        .request_policy
        .is_some_and(|allowed| !allowed(&hop.url, redirects > 0))
    {
        return Err(HttpError::new(
            "Request blocked by native document policy",
            ErrorCode::Network,
        ));
    }
    Ok(())
}

/// Fetch's same-origin mode check: an opaque / tainted / cross-origin redirect
/// aborts the fetch rather than leaking the request to another origin.
fn check_same_origin_mode(opts: &FetchOptions<'_>, hop: &Hop) -> Result<(), HttpError> {
    let violates = opts.context.as_ref().is_some_and(|context| {
        context.mode == crate::request_context::RequestMode::SameOrigin
            && (context.opaque_origin
                || context.origin_tainted
                || context
                    .source
                    .as_deref()
                    .is_none_or(|source| !crate::request_context::same_origin(source, &hop.url)))
    });
    if violates {
        return Err(HttpError::new(
            "Request redirect violates same-origin mode",
            ErrorCode::Network,
        ));
    }
    Ok(())
}

/// Call `opts.response_side_effects` for responses that carry recorded network
/// effects and that would have sent credentials in Fetch terms.
fn notify_response_effects(opts: &FetchOptions<'_>, res: &http::Response) {
    if let (Some(effects), Some(observer)) = (
        res.extensions().get::<NetworkResponseEffects>(),
        opts.response_side_effects,
    ) {
        let context = opts.context.clone().unwrap_or_default();
        if context.sends_credentials(&effects.url) {
            observer(&context, &effects.url, &effects.headers);
        }
    }
}

fn observe_response(opts: &FetchOptions<'_>, hop: &Hop, response: &http::Response) {
    if let Some(observer) = opts.response_observer {
        observer(ResponseObservation {
            url: hop.url.clone(),
            method: hop.method.clone(),
            request_headers: opts.headers.clone(),
            request_body: hop
                .body_bytes
                .clone()
                .or_else(|| hop.body.as_ref().map(|b| b.as_bytes().to_vec())),
            status: response.status().as_u16(),
            status_text: status_text(response),
            headers: response_headers_pairs(response),
        });
    }
}

/// Fetch's response-policy gate — surface the response to the native document
/// policy (CSP / mixed content / etc.) and refuse it if blocked.
fn check_response_policy(
    opts: &FetchOptions<'_>,
    hop: &Hop,
    res: &http::Response,
) -> Result<(), HttpError> {
    let Some(allowed) = opts.response_policy else {
        return Ok(());
    };
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
    Ok(())
}

/// What [`follow_manually_response`] should do with the current response once
/// its redirect location (and the caller's redirect mode) have been inspected.
enum RedirectDispatch {
    /// Return the response as a final (unredirected) answer.
    Return,
    /// Return the response verbatim (`redirect: manual` convention).
    ReturnAsFinal,
    /// Bubble this error out.
    Error(HttpError),
    /// Follow this location as the next hop.
    Follow(String),
}

/// Classify the response for the manual-redirect loop: compare the redirect
/// location (if any) against the caller's `opts.redirect` and the remaining
/// redirect budget, and return the shape of the next step.
fn redirect_dispatch(
    opts: &FetchOptions<'_>,
    redirects: usize,
    max_redirects: usize,
    res: &http::Response,
) -> RedirectDispatch {
    let Some(loc) = redirect_location(res) else {
        return RedirectDispatch::Return;
    };
    if opts.redirect == Some(RedirectMode::Error) {
        return RedirectDispatch::Error(HttpError::new(
            "Request redirect mode is error",
            ErrorCode::Network,
        ));
    }
    if opts.redirect == Some(RedirectMode::Manual) {
        return RedirectDispatch::ReturnAsFinal;
    }
    if redirects < max_redirects {
        return RedirectDispatch::Follow(loc);
    }
    if opts.redirect == Some(RedirectMode::Follow) {
        return RedirectDispatch::Error(HttpError::new(
            "Request redirect limit exceeded",
            ErrorCode::Network,
        ));
    }
    RedirectDispatch::Return
}

/// Apply one redirect: compute the next hop, enforce the streaming-upload
/// invariant, prune headers per the Fetch spec, update the request context,
/// and finalise any pending cache store for the old response body.
async fn apply_redirect_hop(
    opts: &mut FetchOptions<'_>,
    hop: Hop,
    mut res: http::Response,
    loc: &str,
    streaming_upload: bool,
) -> Result<Hop, HttpError> {
    let next = advance(&hop, &res, loc)?;
    if streaming_upload && next.method != "GET" && res.status().as_u16() != 303 {
        return Err(HttpError::new(
            "Cannot replay a streaming request body",
            ErrorCode::Network,
        ));
    }
    prune_redirect_headers(opts, &hop, &next);
    update_redirect_context(opts, &hop, &next, &res);
    if let Some(mut pending) = res.extensions_mut().remove::<PendingStore>() {
        pending.entry.body = read_capped(res, DEFAULT_MAX_BYTES).await?.to_vec();
        pending.cache.store(pending.entry);
    }
    Ok(next)
}

/// Strip the request headers the Fetch spec drops on redirect: `Authorization`
/// on cross-origin redirects, and the five content-* headers when the method
/// changes between hops.
fn prune_redirect_headers(opts: &mut FetchOptions<'_>, hop: &Hop, next: &Hop) {
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
}

/// Carry the request context across the redirect: honour any `Referrer-Policy`
/// header the hop set, then update origin / referrer state for the next URL.
fn update_redirect_context(
    opts: &mut FetchOptions<'_>,
    hop: &Hop,
    next: &Hop,
    res: &http::Response,
) {
    let Some(context) = opts.context.as_mut() else {
        return;
    };
    if let Some(value) = list_header(res, "referrer-policy") {
        if let Some(policy) = crate::referrer_policy::ReferrerPolicy::from_header(&value) {
            context.referrer_policy = policy;
        }
    }
    context.follow_redirect(&hop.url, &next.url);
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
        Outgoing {
            method: &method,
            url,
            headers: &headers,
            body: &opts.body,
            body_bytes: &opts.body_bytes,
            upload: None,
            restart: false,
        },
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
            || opts.dictionary_store.is_some()
            || opts.client_hints.is_some()
            || opts.context.is_some()
            || opts.request_policy.is_some()
            || opts.response_policy.is_some()
            || opts.response_observer.is_some()
            || opts.response_side_effects.is_some()
            || opts.before_request.is_some()
            || opts.http_credentials.is_some() =>
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
    let encoding = dictionary_encoding(&res)?;
    let dictionary_store = res.extensions().get::<PendingDictionary>().cloned();
    let stream: BodyStream = if let Some(encoding) = encoding {
        let dictionary = res
            .extensions()
            .get::<SelectedDictionary>()
            .cloned()
            .ok_or_else(|| {
                HttpError::new(
                    "Dictionary response without an advertised dictionary",
                    ErrorCode::Network,
                )
            })?;
        let transport = Box::pin(res.bytes_stream().map(|chunk| {
            chunk.map_err(|error| {
                crate::compression_dictionary::decode::DecodeError::Invalid(error.to_string())
            })
        }));
        Box::pin(
            crate::compression_dictionary::decode::decode_stream(
                encoding, transport, dictionary, max_bytes,
            )
            .map(|chunk| {
                chunk.map_err(|error| {
                    let code = if matches!(
                        error,
                        crate::compression_dictionary::decode::DecodeError::Size
                    ) {
                        ErrorCode::BodyTooLarge
                    } else {
                        ErrorCode::Network
                    };
                    HttpError::new(format!("Dictionary response decode failed: {error}"), code)
                })
            }),
        )
    } else {
        Box::pin(res.bytes_stream().map(|chunk| {
            chunk.map_err(|error| HttpError::new(error.to_string(), ErrorCode::Network))
        }))
    };
    let body = ResponseBody {
        stream: Some(stream),
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
        dictionary_store,
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
/// an image on purpose); [`fetch_typed_bytes`] also returns the `Content-Type`.
pub async fn fetch_bytes(
    url: &str,
    opts: FetchOptions<'_>,
) -> Result<(String, Vec<u8>, u16), HttpError> {
    fetch_typed_bytes(url, opts)
        .await
        .map(|fetched| (fetched.final_url, fetched.bytes, fetched.status))
}

/// A [`fetch_typed_bytes`] response: raw bytes plus what a resource loader decides with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedBytes {
    pub final_url: String,
    pub bytes: Vec<u8>,
    pub status: u16,
    /// The final response's `Content-Type` header ("" when absent). An `<img>` load
    /// needs it: browsers decode SVG only when it is served as `image/svg+xml`, while
    /// raster formats are sniffed from their bytes.
    pub content_type: String,
}

/// True when any option on `opts` forces the manual redirect / cache / policy
/// path instead of letting the reqwest client follow redirects on its own.
fn needs_manual_redirect_path(opts: &FetchOptions<'_>) -> bool {
    opts.redirect.is_some()
        || opts.max_redirects.is_some()
        || opts.http_cache.is_some()
        || opts.dictionary_store.is_some()
        || opts.client_hints.is_some()
        || opts.context.is_some()
        || opts.request_policy.is_some()
        || opts.response_policy.is_some()
        || opts.response_observer.is_some()
        || opts.response_side_effects.is_some()
        || opts.before_request.is_some()
        || opts.http_credentials.is_some()
}

/// Manual redirect / cache / policy branch of [`fetch_typed_bytes`]: drive
/// [`follow_manually_response`], materialise its body up to `max_bytes`, and
/// stash a pending cache entry if the response carried one.
async fn fetch_typed_bytes_manual(
    opts: &mut FetchOptions<'_>,
    url: &str,
    max_bytes: usize,
) -> Result<FetchedBytes, HttpError> {
    let redirects = opts.max_redirects.unwrap_or(20);
    let (res, final_url, _) = follow_manually_response(opts, url, redirects).await?;
    let status = res.status().as_u16();
    let content_type = header_value(&res, "content-type");
    let cache_store = res.extensions().get::<PendingStore>().cloned();
    let bytes = read_capped(res, max_bytes).await?;
    if let Some(mut pending) = cache_store {
        pending.entry.body = bytes[pending.skip_prefix.min(bytes.len())..].to_vec();
        pending.cache.store(pending.entry);
    }
    Ok(FetchedBytes {
        final_url,
        bytes,
        status,
        content_type,
    })
}

/// Direct branch of [`fetch_typed_bytes`]: borrow the caller's client (or make a
/// default redirect-following one), send one request, and read the response
/// body up to `max_bytes`. No caching / policy hooks fire in this path.
async fn fetch_typed_bytes_direct(
    opts: &mut FetchOptions<'_>,
    url: &str,
    max_bytes: usize,
) -> Result<FetchedBytes, HttpError> {
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
        Outgoing {
            method: &method,
            url,
            headers: &headers,
            body: &opts.body,
            body_bytes: &opts.body_bytes,
            upload: None,
            restart: false,
        },
    )
    .await?;
    let final_url = final_url(&res);
    ingest_set_cookie(opts, &res, &final_url);
    let status = res.status().as_u16();
    let content_type = header_value(&res, "content-type");
    let bytes = read_capped(res, max_bytes).await?;
    Ok(FetchedBytes {
        final_url,
        bytes,
        status,
        content_type,
    })
}

/// [`fetch_bytes`], also returning the final response's `Content-Type`.
pub async fn fetch_typed_bytes(
    url: &str,
    mut opts: FetchOptions<'_>,
) -> Result<FetchedBytes, HttpError> {
    let max_bytes = opts.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    if needs_manual_redirect_path(&opts) {
        return fetch_typed_bytes_manual(&mut opts, url, max_bytes).await;
    }
    fetch_typed_bytes_direct(&mut opts, url, max_bytes).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn transport_failure_preserves_underlying_cause() {
        #[derive(Debug)]
        struct Transport(std::io::Error);
        impl std::fmt::Display for Transport {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("client error (SendRequest)")
            }
        }
        impl std::error::Error for Transport {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let failure = Transport(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "fixture peer reset",
        ));
        let error = super::transport_error(&failure, "http://localhost/script.js");
        assert!(error
            .message
            .contains("client error (SendRequest): fixture peer reset"));
        assert!(error.message.ends_with("(url: http://localhost/script.js)"));
        assert_eq!(error.code, super::ErrorCode::Network);
    }

    #[test]
    fn protocol_memory_drops_priority_only_for_http1_origins() {
        let url = "https://protocol-memory.test/a";
        let headers = BTreeMap::from([("priority".to_owned(), "u=0, i".to_owned())]);
        // Unknown origin: both transport headers, since ALPN has not spoken yet.
        let unknown = super::wire_headers("https://protocol-unknown.test/", &headers, false);
        assert_eq!(unknown["priority"], "u=0, i");
        assert_eq!(unknown["connection"], "keep-alive");
        super::remember_protocol(url, http::Version::HTTP_11);
        let wire = super::wire_headers(url, &headers, false);
        assert!(!wire.contains_key("priority"));
        assert_eq!(wire["connection"], "keep-alive");
        let explicit = BTreeMap::from([("connection".to_owned(), "close".to_owned())]);
        assert_eq!(
            super::wire_headers(url, &explicit, false)["connection"],
            "close"
        );
        super::remember_protocol(url, http::Version::HTTP_2);
        assert_eq!(
            super::wire_headers(url, &headers, false)["priority"],
            "u=0, i"
        );
        super::remember_protocol("data:,x", http::Version::HTTP_11);
        assert_eq!(
            super::known_protocol(url),
            Some(super::OriginProtocol::Http2)
        );
        // One test owns the process-wide map, so clearing it races nothing.
        for n in 0..=super::ORIGIN_PROTOCOL_LIMIT {
            super::remember_protocol(
                &format!("https://h{n}.bounded.test/"),
                http::Version::HTTP_11,
            );
        }
        assert!(super::origin_protocols().lock().unwrap().len() <= super::ORIGIN_PROTOCOL_LIMIT);
    }

    #[test]
    fn http2_hops_never_hand_connection_to_the_encoder() {
        // The h2 encoder strips `Connection` with a `warn!` per request; surf
        // strips it first for origins known to speak HTTP/2 and forced-h2 hops.
        let url = "https://protocol-h2.test/a";
        let headers = BTreeMap::from([
            ("connection".to_owned(), "close".to_owned()),
            ("priority".to_owned(), "u=0, i".to_owned()),
        ]);
        let empty = BTreeMap::new();
        let forced = super::wire_headers("https://protocol-forced.test/", &empty, true);
        assert!(!forced.contains_key("connection"));
        super::remember_protocol(url, http::Version::HTTP_2);
        let wire = super::wire_headers(url, &headers, false);
        assert!(!wire.contains_key("connection"));
        assert_eq!(wire["priority"], "u=0, i");
        assert!(!super::wire_headers(url, &empty, false).contains_key("connection"));
        super::remember_protocol("https://protocol-h3.test/", http::Version::HTTP_3);
        assert!(
            !super::wire_headers("https://protocol-h3.test/x", &empty, false)
                .contains_key("connection")
        );
        // Back on HTTP/1.1 the Chrome keep-alive returns.
        super::remember_protocol(url, http::Version::HTTP_11);
        assert_eq!(
            super::wire_headers(url, &empty, false)["connection"],
            "keep-alive"
        );
    }

    #[tokio::test]
    async fn send_takes_one_outgoing_hop() {
        // The `Outgoing` struct carries every hop field through to the wire.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let mut seen = Vec::new();
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                seen.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&seen).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    if text.len() >= end + 4 + 5 {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            socket
                .write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            String::from_utf8_lossy(&seen).to_ascii_lowercase()
        });
        let url = format!("http://127.0.0.1:{port}/outgoing");
        let headers = BTreeMap::from([("x-hop".to_owned(), "1".to_owned())]);
        let cl = client(http::redirect::Policy::none()).unwrap();
        let res = send(
            &cl,
            Outgoing {
                method: "PUT",
                url: &url,
                headers: &headers,
                body: &None,
                body_bytes: &Some(b"hello".to_vec()),
                upload: None,
                restart: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(res.status().as_u16(), 201);
        let raw = server.await.unwrap();
        assert!(raw.starts_with("put /outgoing http/1.1"), "{raw}");
        assert!(raw.contains("x-hop: 1"), "{raw}");
        assert!(raw.contains("connection: keep-alive"), "{raw}");
        assert!(raw.ends_with("hello"), "{raw}");
    }

    use super::*;

    #[test]
    fn cors_preflights_carry_no_client_hints() {
        let preflight = FetchOptions {
            headers: BTreeMap::from([("access-control-request-method".into(), "PUT".into())]),
            ..Default::default()
        };
        let headers = build_headers("https://a.test/r", "OPTIONS", &preflight);
        assert!(!headers.keys().any(|name| name.starts_with("sec-ch-")));
        assert_eq!(headers["access-control-request-method"], "PUT");
        let get = build_headers("https://a.test/r", "GET", &FetchOptions::default());
        assert!(get.contains_key("sec-ch-ua"));
    }

    #[cfg(feature = "impersonate")]
    #[test]
    fn every_registry_release_names_a_supported_transport_recipe() {
        for release in crate::browser::RELEASES {
            transport_recipe(release.transport_base_major);
        }
    }

    #[cfg(feature = "impersonate")]
    #[test]
    fn per_request_order_follows_the_measured_class() {
        let headers = BTreeMap::from([
            ("sec-fetch-mode".to_string(), "no-cors".to_string()),
            ("sec-fetch-dest".to_string(), "image".to_string()),
        ]);
        let order = request_order("GET", &headers, false);
        match crate::browser::default_release().request_order("no-cors") {
            Some(expected) => {
                let names: Vec<String> = order
                    .unwrap()
                    .iter()
                    .map(|(name, _)| name.as_str().to_string())
                    .collect();
                let expected: Vec<String> = expected
                    .iter()
                    .map(|name| name.to_ascii_lowercase())
                    .collect();
                assert_eq!(names, expected);
            }
            None => assert!(order.is_none()),
        }
    }

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
    async fn host_body_observation_completes_without_guest_pulls_or_cache_admission() {
        let port = spawn().await;
        let response = fetch_stream(&url(port, "/"), FetchOptions::default())
            .await
            .unwrap();
        let (_owner, receiver) = tokio::sync::watch::channel(false);
        let bytes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = bytes.clone();
        let (completed, completion) = tokio::sync::oneshot::channel();
        let completed = std::sync::Mutex::new(Some(completed));
        let mut body = response.body.background_observed(
            receiver,
            std::sync::Arc::new(move |chunk| captured.lock().unwrap().extend_from_slice(chunk)),
            std::sync::Arc::new(move |result| {
                if let Some(sender) = completed.lock().unwrap().take() {
                    let _ = sender.send(result);
                }
            }),
        );
        assert!(
            !body.detach_cache_consumer(),
            "observation is not cache admission"
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), completion)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&bytes.lock().unwrap()).contains("ok"));
        let mut consumed = Vec::new();
        while let Some(chunk) = body.next_chunk().await.unwrap() {
            consumed.extend(chunk);
        }
        assert_eq!(consumed, *bytes.lock().unwrap());
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
        let observed = std::sync::Mutex::new(Vec::new());
        let observer = |hop: ResponseObservation| observed.lock().unwrap().push(hop);
        let mut jar = CookieJar::new();
        let mut response = fetch_stream(
            &target,
            FetchOptions {
                allow_non_html: true,
                jar: Some(&mut jar),
                request_policy: Some(&policy),
                response_policy: Some(&responses),
                response_observer: Some(&observer),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            observed
                .lock()
                .unwrap()
                .iter()
                .map(|hop| hop.status)
                .collect::<Vec<_>>(),
            vec![302, 200]
        );
        {
            let hops = observed.lock().unwrap();
            assert!(hops[0].url.ends_with("/start"));
            assert!(hops[1].url.ends_with("/finish"));
            assert_eq!(hops[0].status_text, "Found");
            assert!(hops[0]
                .headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("location") && value == "/finish"));
        }
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
        let observed = std::sync::Mutex::new(Vec::new());
        let observer = |hop: ResponseObservation| observed.lock().unwrap().push(hop);
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
                response_observer: Some(&observer),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            result.unwrap_err().message,
            "Response blocked by native document policy"
        );
        assert!(observed.lock().unwrap().is_empty());
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
    async fn typed_bytes_report_the_final_content_type_on_both_paths() {
        let p = spawn().await;
        let direct = fetch_typed_bytes(&url(p, "/json"), FetchOptions::default())
            .await
            .unwrap();
        assert_eq!(
            (
                direct.status,
                direct.bytes.as_slice(),
                direct.content_type.as_str()
            ),
            (200, b"{}".as_slice(), "application/json")
        );
        // The manual-follow path (a request policy) reports the redirect target's type.
        let allow = |_: &str, _: bool| true;
        let followed = fetch_typed_bytes(
            &url(p, "/r"),
            FetchOptions {
                request_policy: Some(&allow),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(followed.final_url.ends_with("/dest"));
        assert_eq!(followed.content_type, "text/html");
        let auto = fetch_typed_bytes(&url(p, "/r"), FetchOptions::default())
            .await
            .unwrap();
        assert_eq!(
            fetch_bytes(&url(p, "/r"), FetchOptions::default())
                .await
                .unwrap(),
            (auto.final_url, auto.bytes, auto.status)
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

    #[test]
    fn consent_cookies_merge_without_overriding_an_explicit_cookie() {
        let google = "https://www.google.com/search?q=1";
        let mut empty = BTreeMap::new();
        seed_consent_cookies(&mut empty, google, None);
        assert_eq!(
            empty.get("cookie").map(String::as_str),
            Some(format!("SOCS={}", crate::consent::DEFAULT_SOCS).as_str())
        );
        let mut other = BTreeMap::from([("cookie".to_string(), "a=1".to_string())]);
        seed_consent_cookies(&mut other, google, Some("CUSTOM"));
        assert_eq!(other["cookie"], "a=1; SOCS=CUSTOM");
        let mut mine = BTreeMap::from([("cookie".to_string(), "a=1; SOCS = mine".to_string())]);
        seed_consent_cookies(&mut mine, google, Some("CUSTOM"));
        assert_eq!(mine["cookie"], "a=1; SOCS = mine");
        let mut none = BTreeMap::new();
        seed_consent_cookies(&mut none, google, Some(""));
        seed_consent_cookies(&mut none, "https://example.test/", None);
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn manual_redirects_enforce_the_limit_and_same_origin_mode() {
        let port = spawn().await;
        let limited = fetch_html(
            &url(port, "/r"),
            FetchOptions {
                redirect: Some(RedirectMode::Follow),
                max_redirects: Some(0),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            limited.to_string(),
            "Network: Request redirect limit exceeded"
        );
        let followed = fetch_html(
            &url(port, "/r"),
            FetchOptions {
                redirect: Some(RedirectMode::Follow),
                max_redirects: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(followed.html, "<html>dest</html>");
        // A same-origin-mode request may not be redirected to another origin.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bounce = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf).await;
                let resp = format!("HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/dest\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        let source = url(bounce, "/page");
        let same_origin = fetch_html(
            &url(bounce, "/go"),
            FetchOptions {
                context: Some(RequestContext::resource(
                    &source,
                    crate::request_context::Destination::Empty,
                    crate::request_context::RequestMode::SameOrigin,
                )),
                max_redirects: Some(5),
                allow_non_html: true,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            same_origin.to_string(),
            "Network: Request redirect violates same-origin mode"
        );
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
            dictionary_store: None,
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

#[cfg(test)]
mod edge_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Serves `responses` in order and returns each request head it read.
    async fn serve(
        responses: Vec<&'static [u8]>,
    ) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (sent, heads) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut head = Vec::new();
                while !head.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut bytes = [0; 4096];
                    let n = socket.read(&mut bytes).await.unwrap();
                    head.extend_from_slice(&bytes[..n]);
                }
                let _ = sent.send(String::from_utf8_lossy(&head).to_ascii_lowercase());
                socket.write_all(response).await.unwrap();
            }
        });
        (origin, heads)
    }

    // Fetch negotiates a Range request's Accept-Encoding itself (identity, plus
    // dcb/dcz with a selected dictionary; Chrome 156 `header-range-request`).
    // The transport must send that value, not substitute its own.
    #[tokio::test]
    async fn the_transport_keeps_a_range_requests_negotiated_accept_encoding() {
        const PARTIAL: &[u8] = b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-1/4\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
        let (origin, mut heads) = serve(vec![PARTIAL, PARTIAL]).await;
        let client = build_client();
        for value in ["identity, dcb, dcz", "identity"] {
            let headers = BTreeMap::from([
                ("range".to_owned(), "bytes=0-1".to_owned()),
                ("accept-encoding".to_owned(), value.to_owned()),
            ]);
            let url = format!("{origin}/ranged");
            send(
                &client,
                Outgoing {
                    method: "GET",
                    url: &url,
                    headers: &headers,
                    body: &None,
                    body_bytes: &None,
                    upload: None,
                    restart: false,
                },
            )
            .await
            .unwrap();
            let head = heads.recv().await.unwrap();
            assert!(
                head.contains(&format!("\r\naccept-encoding: {value}\r\n")),
                "{value}: {head}"
            );
        }
    }

    #[cfg(feature = "impersonate")]
    #[test]
    fn the_transport_reuses_the_chrome_154_recipe() {
        assert_eq!(transport_base_chrome_major(), Some(154));
        assert_eq!(
            transport_base_chrome_major(),
            Some(crate::browser::default_release().transport_base_major)
        );
        assert!(matches!(
            transport_recipe(154),
            wreq_util::Profile::Chrome154
        ));
    }

    #[cfg(feature = "impersonate")]
    #[test]
    #[should_panic(expected = "browser_releases.json names unsupported transport recipe Chrome155")]
    fn an_unknown_transport_recipe_is_a_registry_error() {
        let _ = transport_recipe(155);
    }

    #[tokio::test]
    async fn a_method_changing_redirect_drops_request_body_headers() {
        // Fetch 4.4 HTTP-redirect fetch step 12: a POST turned into GET removes the
        // request-body header names; other author headers stay.
        let (origin, mut heads) = serve(vec![
            b"HTTP/1.1 303 See Other\r\nLocation: /landing\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ])
        .await;
        let mut headers = BTreeMap::new();
        for (name, value) in [
            ("content-type", "text/plain"),
            ("content-language", "en"),
            ("content-location", "/x"),
            ("content-encoding", "identity"),
            ("x-keep", "1"),
        ] {
            headers.insert(name.to_owned(), value.to_owned());
        }
        let response = fetch_stream(
            &format!("{origin}/form"),
            FetchOptions {
                method: Some("POST".into()),
                body: Some("a=1".into()),
                headers,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(response.final_url, format!("{origin}/landing"));
        let post = heads.recv().await.unwrap();
        assert!(post.starts_with("post /form "), "{post}");
        assert!(post.contains("content-language: en\r\n"), "{post}");
        let get = heads.recv().await.unwrap();
        assert!(get.starts_with("get /landing "), "{get}");
        for gone in [
            "content-type",
            "content-language",
            "content-location",
            "content-encoding",
        ] {
            assert!(!get.contains(&format!("{gone}:")), "{gone} in {get}");
        }
        assert!(get.contains("x-keep: 1\r\n"), "{get}");
    }

    #[tokio::test]
    async fn streaming_uploads_need_https_and_the_upload_pool() {
        let (_sender, upload) = crate::upload::upload_channel(16);
        let error = fetch_stream(
            "http://127.0.0.1:1/upload",
            FetchOptions {
                method: Some("POST".into()),
                upload_body: Some(upload.clone()),
                ..Default::default()
            },
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.message, "Streaming requests require secure HTTP/2");
        // An https target builds the dedicated upload pool, then fails to connect.
        let error = fetch_stream(
            "https://127.0.0.1:1/upload",
            FetchOptions {
                method: Some("POST".into()),
                upload_body: Some(upload),
                ..Default::default()
            },
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.code, ErrorCode::Network);
        assert!(
            error.message.contains("(url: https://127.0.0.1:1/upload)"),
            "{}",
            error.message
        );
        // With session client hints the session's upload pool is used instead.
        let hints = ClientHints::default();
        let (_sender, upload) = crate::upload::upload_channel(16);
        let error = fetch_stream(
            "https://127.0.0.1:1/hinted",
            FetchOptions {
                method: Some("POST".into()),
                upload_body: Some(upload),
                client_hints: Some(&hints),
                ..Default::default()
            },
        )
        .await
        .err()
        .unwrap();
        assert!(
            error.message.contains("(url: https://127.0.0.1:1/hinted)"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn clear_site_data_cache_clears_the_dictionary_store_and_dcb_needs_a_dictionary() {
        let (origin, _heads) = serve(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            b"HTTP/1.1 200 OK\r\nClear-Site-Data: \"cookies\", \"cache\"\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Encoding: dcb\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ])
        .await;
        let store = DictionaryStore::default();
        let generation = store.generation();
        for (path, cleared) in [("/plain", 0), ("/clear", 1)] {
            fetch_stream(
                &format!("{origin}{path}"),
                FetchOptions {
                    dictionary_store: Some(&store),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert_eq!(store.generation(), generation + cleared, "{path}");
        }
        // Content-Encoding: dcb without an advertised dictionary is a network error.
        let error = fetch_stream(&format!("{origin}/dcb"), FetchOptions::default())
            .await
            .err()
            .unwrap();
        assert_eq!(
            error.message,
            "Dictionary response without an advertised dictionary"
        );
    }

    #[tokio::test]
    async fn a_streamed_dictionary_is_registered_only_within_eight_mib() {
        use crate::compression_dictionary::MAX_DICTIONARY_BYTES;
        use crate::request_context::{Destination, RequestMode};
        let context =
            RequestContext::resource("https://dict.test/", Destination::Script, RequestMode::Cors);
        for (extra, stored) in [(0, true), (1, false)] {
            let store = DictionaryStore::default();
            let pending = PendingDictionary::new(
                &store,
                store.generation(),
                "p",
                "https://dict.test/d",
                &context,
                200,
                vec![
                    ("use-as-dictionary".into(), "match=\"/*\"".into()),
                    ("cache-control".into(), "max-age=60".into()),
                ],
                std::time::SystemTime::now(),
            );
            let chunks = vec![
                Ok(bytes::Bytes::from(vec![1; MAX_DICTIONARY_BYTES - 1])),
                Ok(bytes::Bytes::from(vec![2; 1 + extra])),
            ];
            let mut body = ResponseBody {
                stream: Some(Box::pin(futures_util::stream::iter(chunks))),
                received: 0,
                max_bytes: usize::MAX,
                trace_id: 0,
                url: "https://dict.test/d".into(),
                status: 200,
                content_type: String::new(),
                trace_body: None,
                trace_prefix: 0,
                trace_skip_prefix: 0,
                cache_store: None,
                dictionary_store: pending,
                background: None,
            };
            let mut received = 0;
            while let Some(chunk) = body.next_chunk().await.unwrap() {
                received += chunk.len();
            }
            assert_eq!(received, MAX_DICTIONARY_BYTES + extra);
            let selected = store.select("p", "https://dict.test/a", &context);
            assert_eq!(selected.is_some(), stored, "extra {extra}");
        }
    }

    #[tokio::test]
    async fn a_background_fill_honours_a_closed_owner_and_a_detached_consumer() {
        const CACHEABLE: &[u8] = b"HTTP/1.1 200 OK\r\nCache-Control: max-age=60\r\nContent-Type: text/html\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
        let (origin, _heads) = serve(vec![CACHEABLE, CACHEABLE]).await;
        let cache = HttpCache::default();
        let options = |mode| FetchOptions {
            http_cache: Some(&cache),
            cache_mode: mode,
            ..Default::default()
        };
        // An owner that is already gone cancels the fill before any transport read.
        let (_owner, closed) = tokio::sync::watch::channel(true);
        let mut body = fetch_stream(&format!("{origin}/owned"), options(CacheMode::Default))
            .await
            .unwrap()
            .body
            .background_cache(closed);
        let error = body.next_chunk().await.err().unwrap();
        assert_eq!(error.message, "Response body canceled");
        assert_eq!(body.background_status(), Some(false));
        assert!(
            fetch_stream(&format!("{origin}/owned"), options(CacheMode::OnlyIfCached))
                .await
                .is_err()
        );
        // A detached consumer queues nothing, but the native fill still completes
        // and every transport chunk still reaches the observer.
        let (_owner, open) = tokio::sync::watch::channel(false);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let observer: ResponseChunkObserver =
            std::sync::Arc::new(move |bytes: &[u8]| sink.lock().unwrap().extend_from_slice(bytes));
        let mut body = fetch_stream(&format!("{origin}/detached"), options(CacheMode::Default))
            .await
            .unwrap()
            .body
            .background_cache_with_observer(open, Some(observer));
        assert!(body.detach_cache_consumer());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !body.background_complete() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(body.background_status(), Some(true));
        assert_eq!(*seen.lock().unwrap(), b"hello");
        assert!(body
            .background
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap()
            .chunks
            .is_empty());
        let mut cached = fetch_stream(
            &format!("{origin}/detached"),
            options(CacheMode::OnlyIfCached),
        )
        .await
        .unwrap();
        assert_eq!(cached.body.next_chunk().await.unwrap().unwrap(), b"hello");
    }
}

#[cfg(test)]
mod http_auth_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn browser_credentials_retry_basic_once_and_respect_scope_and_omission() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let requests = seen.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    let mut buffer = [0; 4096];
                    let len = socket.read(&mut buffer).await.unwrap();
                    if len == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..len]);
                }
                let request = String::from_utf8_lossy(&bytes).to_lowercase();
                requests.lock().unwrap().push(request.clone());
                let response = if request.contains("authorization: basic ywxpy2u6c2vjcmv0") {
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK"
                } else {
                    "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=probe\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nNO"
                };
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let credentials = crate::http_auth::Credentials {
            username: "alice".into(),
            password: "secret".into(),
            origin: Some(origin.clone()),
        };
        let observed = std::sync::Mutex::new(Vec::new());
        let observer = |hop: ResponseObservation| observed.lock().unwrap().push(hop);
        let response = fetch_html(
            &origin,
            FetchOptions {
                response_observer: Some(&observer),
                http_credentials: Some(&credentials),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.html, "OK");
        assert_eq!(
            observed
                .lock()
                .unwrap()
                .iter()
                .map(|hop| hop.status)
                .collect::<Vec<_>>(),
            vec![401, 200]
        );
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(!seen.lock().unwrap()[0].contains("authorization:"));
        seen.lock().unwrap().clear();
        let other = crate::http_auth::Credentials {
            origin: Some("http://127.0.0.1:1".into()),
            ..credentials.clone()
        };
        assert_eq!(
            fetch_html(
                &origin,
                FetchOptions {
                    http_credentials: Some(&other),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .status,
            401
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
        seen.lock().unwrap().clear();
        let context = RequestContext {
            credentials: crate::request_context::Credentials::Omit,
            ..Default::default()
        };
        assert_eq!(
            fetch_html(
                &origin,
                FetchOptions {
                    http_credentials: Some(&credentials),
                    context: Some(context),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .status,
            401
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
        server.abort();
    }
}
