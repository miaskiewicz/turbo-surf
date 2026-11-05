//! Enforced fetch and inline-script CSP subset; unsupported directives are not
//! represented as implemented. Header policies intersect, as do encountered meta policies.
use base64::Engine;
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::{collections::BTreeMap, sync::Arc};
use turbo_surf_core::request_context::same_origin;

type Directives = BTreeMap<String, Vec<String>>;

fn has(list: &[String], token: &str) -> bool {
    list.iter().any(|source| source == token)
}

/// Who inserted a `<script>` element: `'strict-dynamic'` trusts only scripts that
/// another script inserted, never parser-inserted ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScriptInsertion {
    #[default]
    Parser,
    Script,
}

/// A `<script>` element's CSP check: inline (`url` empty, `text` its source) or external.
pub struct Script<'a> {
    pub url: &'a str,
    pub text: &'a str,
    pub nonce: &'a str,
    pub insertion: ScriptInsertion,
}

struct Request<'a> {
    kind: &'a str,
    target: &'a str,
    nonce: &'a str,
    insertion: ScriptInsertion,
    redirected: bool,
}
impl<'a> Request<'a> {
    fn new(
        kind: &'a str,
        target: &'a str,
        nonce: &'a str,
        insertion: ScriptInsertion,
        redirected: bool,
    ) -> Self {
        Self {
            kind,
            target,
            nonce,
            insertion,
            redirected,
        }
    }
}

struct Inline<'a> {
    kind: &'a str,
    hashes: [String; 3],
    nonce: &'a str,
    insertion: ScriptInsertion,
}
impl<'a> Inline<'a> {
    fn new(kind: &'a str, text: &str, nonce: &'a str, insertion: ScriptInsertion) -> Self {
        let hash = |algorithm: &str, bytes: Vec<u8>| {
            format!(
                "'{algorithm}-{}'",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        };
        Self {
            kind,
            hashes: [
                hash("sha256", Sha256::digest(text.as_bytes()).to_vec()),
                hash("sha384", Sha384::digest(text.as_bytes()).to_vec()),
                hash("sha512", Sha512::digest(text.as_bytes()).to_vec()),
            ],
            nonce,
            insertion,
        }
    }
}

#[derive(Clone)]
pub struct ReportRecord {
    pub original: String,
    pub sample: bool,
    pub destination: ReportDestination,
}
#[derive(Clone, Default)]
pub enum ReportDestination {
    #[default]
    None,
    Legacy(Vec<String>),
    Group(String),
}
#[derive(Clone, Default)]
pub struct Policy {
    origin: Arc<str>,
    policies: Arc<Vec<BTreeMap<String, Vec<String>>>>,
    originals: Arc<Vec<String>>,
    header_reports: Arc<Vec<bool>>,
    form_action: Option<Arc<FormActionGate>>,
}
/// A form submission's `form-action`, carried by the navigation request it starts in
/// a child frame so every redirect hop answers to the submitting document (Chrome 156
/// cancels the navigation at the refused hop).
#[derive(Default)]
pub struct FormActionGate {
    policy: Policy,
    refused: std::sync::Mutex<Option<String>>,
}
impl FormActionGate {
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            refused: Default::default(),
        }
    }
    pub fn policy(&self) -> &Policy {
        &self.policy
    }
    /// The first URL `form-action` refused, if any.
    pub fn refused(&self) -> Option<String> {
        self.refused.lock().unwrap().clone()
    }
    fn allows(&self, target: &str, redirected: bool) -> bool {
        let allowed = self
            .policy
            .allows_request("form-action", target, "", redirected);
        if !allowed {
            self.refused
                .lock()
                .unwrap()
                .get_or_insert_with(|| target.to_string());
        }
        allowed
    }
}
impl Policy {
    pub fn new(origin: &str, values: impl IntoIterator<Item = String>) -> Self {
        let originals: Vec<String> = values
            .into_iter()
            .flat_map(|value| {
                value
                    .split(',')
                    .map(|value| value.trim().to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        let policies = originals
            .iter()
            .map(|value| {
                let mut directives = BTreeMap::new();
                for directive in value.split(';') {
                    let mut words = directive.split_ascii_whitespace();
                    if let Some(name) = words.next() {
                        directives
                            .entry(name.to_ascii_lowercase())
                            .or_insert_with(|| words.map(str::to_owned).collect());
                    }
                }
                directives
            })
            .collect();
        Self {
            origin: origin.into(),
            policies: Arc::new(policies),
            header_reports: Arc::new(vec![true; originals.len()]),
            originals: Arc::new(originals),
            form_action: None,
        }
    }
    /// This policy for a navigation request a form submission started: frame-src
    /// checks (the initial URL and each redirect hop) also pass through `gate`.
    pub fn with_form_action(&self, gate: Arc<FormActionGate>) -> Self {
        Self {
            form_action: Some(gate),
            ..self.clone()
        }
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn add(&mut self, value: &str) {
        let added = Self::new(&self.origin, [value.to_string()]);
        Arc::make_mut(&mut self.header_reports).extend(vec![false; added.policies.len()]);
        Arc::make_mut(&mut self.policies).extend(added.policies.iter().cloned());
        Arc::make_mut(&mut self.originals).extend(added.originals.iter().cloned());
    }
    pub fn inherit_headers(&mut self, inherited: &Self) {
        let reports = std::mem::take(&mut self.header_reports);
        let own = std::mem::take(&mut self.policies);
        let originals = std::mem::take(&mut self.originals);
        *self = inherited.clone();
        Arc::make_mut(&mut self.header_reports).extend(reports.iter().copied());
        Arc::make_mut(&mut self.policies).extend(own.iter().cloned());
        Arc::make_mut(&mut self.originals).extend(originals.iter().cloned());
    }
    /// The directive governing `kind`: CSP3's effective-directive fallback chain.
    fn list<'a>(policy: &'a Directives, kind: &str) -> Option<&'a [String]> {
        let fallbacks: &[&str] = match kind {
            "base-uri" | "form-action" => &[],
            "script-src-elem" | "script-src-attr" => &["script-src", "default-src"],
            "style-src-elem" | "style-src-attr" => &["style-src", "default-src"],
            "worker-src" => &["child-src", "script-src", "default-src"],
            "frame-src" => &["child-src", "default-src"],
            _ => &["default-src"],
        };
        std::iter::once(kind)
            .chain(fallbacks.iter().copied())
            .find_map(|name| policy.get(name))
            .map(Vec::as_slice)
    }
    /// Report records for each policy `violated` refuses; it answers with the
    /// record's `sample` flag.
    fn records(&self, mut violated: impl FnMut(&Directives) -> Option<bool>) -> Vec<ReportRecord> {
        self.policies
            .iter()
            .zip(self.originals.iter())
            .zip(self.header_reports.iter())
            .filter_map(|((policy, original), header)| {
                violated(policy)
                    .map(|sample| Self::report_record(policy, original, sample, *header))
            })
            .collect()
    }
    fn request_allowed(&self, policy: &Directives, request: &Request) -> bool {
        let Some(list) = Self::list(policy, request.kind) else {
            return true;
        };
        if !request.nonce.is_empty()
            && matches!(request.kind, "script-src-elem" | "style-src-elem")
            && has(list, &format!("'nonce-{}'", request.nonce))
        {
            return true;
        }
        // CSP3 8.2: 'strict-dynamic' drops host/scheme sources and 'self'; only a
        // script another script inserted is trusted.
        if request.kind == "script-src-elem" && has(list, "'strict-dynamic'") {
            return request.insertion == ScriptInsertion::Script;
        }
        list.iter()
            .any(|source| matches_source(source, &self.origin, request.target, request.redirected))
    }
    fn inline_allowed(policy: &Directives, inline: &Inline) -> bool {
        let Some(list) = Self::list(policy, inline.kind) else {
            return true;
        };
        let attribute = inline.kind.ends_with("-attr");
        let dynamic = inline.kind.starts_with("script-src") && has(list, "'strict-dynamic'");
        // Chrome 156 runs a script-inserted inline script under 'strict-dynamic'.
        if dynamic && !attribute && inline.insertion == ScriptInsertion::Script {
            return true;
        }
        let protected = dynamic
            || list.iter().any(|source| {
                ["'nonce-", "'sha256-", "'sha384-", "'sha512-"]
                    .iter()
                    .any(|prefix| source.starts_with(prefix))
            });
        (!attribute && !inline.nonce.is_empty() && has(list, &format!("'nonce-{}'", inline.nonce)))
            || ((!attribute || has(list, "'unsafe-hashes'"))
                && list.iter().any(|source| inline.hashes.contains(source)))
            || (!protected && has(list, "'unsafe-inline'"))
    }
    pub fn allows_request(&self, kind: &str, target: &str, nonce: &str, redirected: bool) -> bool {
        if kind == "frame-src"
            && self
                .form_action
                .as_ref()
                .is_some_and(|gate| !gate.allows(target, redirected))
        {
            return false;
        }
        let request = Request::new(kind, target, nonce, ScriptInsertion::Script, redirected);
        self.policies
            .iter()
            .all(|policy| self.request_allowed(policy, &request))
    }
    pub fn request_violations(&self, kind: &str, url: &str, nonce: &str) -> Vec<ReportRecord> {
        self.request_violations_for(kind, url, nonce, ScriptInsertion::Script)
    }
    /// [`Self::request_violations`] for a script element inserted by `insertion`.
    pub fn request_violations_for(
        &self,
        kind: &str,
        url: &str,
        nonce: &str,
        insertion: ScriptInsertion,
    ) -> Vec<ReportRecord> {
        let request = Request::new(kind, url, nonce, insertion, false);
        self.records(|policy| (!self.request_allowed(policy, &request)).then_some(false))
    }
    pub fn inline_violations(&self, kind: &str, text: &str, nonce: &str) -> Vec<ReportRecord> {
        let inline = Inline::new(kind, text, nonce, ScriptInsertion::Parser);
        self.inline_records(&inline)
    }
    fn inline_records(&self, inline: &Inline) -> Vec<ReportRecord> {
        self.records(|policy| {
            (!Self::inline_allowed(policy, inline)).then(|| {
                Self::list(policy, inline.kind).is_some_and(|list| has(list, "'report-sample'"))
            })
        })
    }
    /// Whether a `<script>` element may run: inline (`url` empty) or external.
    pub fn allows_script(&self, script: &Script) -> bool {
        if script.url.is_empty() {
            let inline = Inline::new(
                "script-src-elem",
                script.text,
                script.nonce,
                script.insertion,
            );
            return self
                .policies
                .iter()
                .all(|policy| Self::inline_allowed(policy, &inline));
        }
        let request = Request::new(
            "script-src-elem",
            script.url,
            script.nonce,
            script.insertion,
            false,
        );
        self.policies
            .iter()
            .all(|policy| self.request_allowed(policy, &request))
    }
    /// The report records for a `<script>` element [`Self::allows_script`] refuses.
    pub fn script_violations(&self, script: &Script) -> Vec<ReportRecord> {
        if script.url.is_empty() {
            let inline = Inline::new(
                "script-src-elem",
                script.text,
                script.nonce,
                script.insertion,
            );
            return self.inline_records(&inline);
        }
        self.request_violations_for(
            "script-src-elem",
            script.url,
            script.nonce,
            script.insertion,
        )
    }
    pub fn eval_violations(&self) -> Vec<ReportRecord> {
        self.records(|policy| {
            let list = Self::list(policy, "script-src")?;
            (!has(list, "'unsafe-eval'")).then(|| has(list, "'report-sample'"))
        })
    }
    fn report_record(
        policy: &BTreeMap<String, Vec<String>>,
        original: &str,
        sample: bool,
        header: bool,
    ) -> ReportRecord {
        let destination = if let Some(tokens) = policy.get("report-to") {
            ReportDestination::Group(tokens.first().cloned().unwrap_or_default())
        } else if !header {
            ReportDestination::None
        } else if let Some(tokens) = policy.get("report-uri") {
            ReportDestination::Legacy(tokens.clone())
        } else {
            ReportDestination::None
        };
        ReportRecord {
            original: original.into(),
            sample,
            destination,
        }
    }
    pub fn sandbox_restrictions(&self) -> Option<(bool, bool)> {
        let mut restrictions = None;
        for policy in self.policies.iter() {
            if let Some(tokens) = policy.get("sandbox") {
                let scripts = !tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("allow-scripts"));
                let origin = !tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("allow-same-origin"));
                let (prior_scripts, prior_origin) = restrictions.unwrap_or((false, false));
                restrictions = Some((scripts || prior_scripts, origin || prior_origin));
            }
        }
        restrictions
    }
    pub fn blocks_form_submission(&self) -> bool {
        self.policies.iter().any(|policy| {
            policy.get("sandbox").is_some_and(|tokens| {
                !tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("allow-forms"))
            })
        })
    }
    pub fn blocks_pointer_lock(&self) -> bool {
        self.policies.iter().any(|policy| {
            policy.get("sandbox").is_some_and(|tokens| {
                !tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("allow-pointer-lock"))
            })
        })
    }
    pub fn blocks_top_navigation_without_activation(&self) -> bool {
        self.policies.iter().any(|policy| {
            policy.get("sandbox").is_some_and(|tokens| {
                !tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("allow-top-navigation"))
            })
        })
    }
    pub fn blocks_storage_access(&self) -> bool {
        self.policies.iter().any(|policy| {
            policy.get("sandbox").is_some_and(|tokens| {
                !tokens.iter().any(|token| {
                    token.eq_ignore_ascii_case("allow-storage-access-by-user-activation")
                })
            })
        })
    }
    pub fn has_frame_ancestors(&self) -> bool {
        self.policies
            .iter()
            .any(|policy| policy.contains_key("frame-ancestors"))
    }
    pub fn allows_ancestors(&self, ancestors: &[String]) -> bool {
        self.policies.iter().all(|policy| {
            policy.get("frame-ancestors").is_none_or(|sources| {
                ancestors.iter().all(|ancestor| {
                    sources
                        .iter()
                        .any(|source| matches_source(source, &self.origin, ancestor, false))
                })
            })
        })
    }
    /// Inline content as the parser inserted it (`'strict-dynamic'` grants it nothing).
    pub fn allows_inline(&self, kind: &str, text: &str, nonce: &str) -> bool {
        let inline = Inline::new(kind, text, nonce, ScriptInsertion::Parser);
        self.policies
            .iter()
            .all(|policy| Self::inline_allowed(policy, &inline))
    }
    pub fn unsupported(&self) -> Vec<String> {
        const SUPPORTED: &[&str] = &[
            "base-uri",
            "frame-ancestors",
            "default-src",
            "script-src",
            "script-src-elem",
            "script-src-attr",
            "style-src",
            "style-src-elem",
            "style-src-attr",
            "connect-src",
            "img-src",
            "frame-src",
            "child-src",
            "worker-src",
            "form-action",
            "report-uri",
            "report-to",
            "trusted-types",
        ];
        let mut limitations = std::collections::BTreeSet::new();
        for policy in self.policies.iter() {
            for (name, values) in policy {
                let native_trusted_sink =
                    cfg!(turbo_surf_native_proxy) && name == "require-trusted-types-for";
                if !SUPPORTED.contains(&name.as_str()) && !native_trusted_sink {
                    limitations.insert(name.clone());
                }
                for value in values {
                    if matches!(
                        value.as_str(),
                        "'wasm-unsafe-eval'" | "'inline-speculation-rules'"
                    ) {
                        limitations.insert(format!("{name}: {value}"));
                    }
                    if value == "'trusted-types-eval'" && !cfg!(turbo_surf_native_proxy) {
                        limitations.insert(format!("{name}: {value}"));
                    }
                }
            }
        }
        limitations.into_iter().collect()
    }

    /// Only explicit Trusted Types directives apply; default-src is not a fallback.
    pub fn requires_trusted_types(&self) -> bool {
        self.policies.iter().any(|policy| {
            policy
                .get("require-trusted-types-for")
                .is_some_and(|tokens| tokens.iter().any(|token| token == "'script'"))
        })
    }
    pub fn trusted_type_violations(&self, name: Option<(&str, bool)>) -> Vec<ReportRecord> {
        self.records(|policy| {
            let violation = if let Some((name, duplicate)) = name {
                policy.get("trusted-types").is_some_and(|tokens| {
                    !tokens.iter().any(|token| token == name || token == "*")
                        || duplicate && (name == "default" || !has(tokens, "'allow-duplicates'"))
                })
            } else {
                policy
                    .get("require-trusted-types-for")
                    .is_some_and(|tokens| has(tokens, "'script'"))
            };
            violation.then_some(true)
        })
    }

    pub fn has_policy(&self) -> bool {
        !self.policies.is_empty()
    }

    pub fn bypasses_trusted_types_eval(&self) -> bool {
        let mut has_keyword = false;
        self.policies.iter().all(|policy| {
            Self::list(policy, "script-src").is_none_or(|list| {
                let trusted = list.iter().any(|token| token == "'trusted-types-eval'");
                has_keyword |= trusted;
                trusted || list.iter().any(|token| token == "'unsafe-eval'")
            })
        }) && has_keyword
    }
    #[cfg(turbo_surf_native_proxy)]
    pub fn eval_error_message(&self) -> Option<String> {
        self.policies.iter().find_map(|policy| {
            let list = Self::list(policy, "script-src")?;
            if list.iter().any(|source| source == "'unsafe-eval'") { return None; }
            let directive = if policy.contains_key("script-src") { "script-src" } else { "default-src" };
            Some(format!("Evaluating a string as JavaScript violates the following Content Security Policy directive because 'unsafe-eval' is not an allowed source of script: {directive} {}\".\n",list.join(" ")))
        })
    }

    pub fn allows_eval(&self) -> bool {
        self.policies.iter().all(|policy| {
            Self::list(policy, "script-src")
                .is_none_or(|list| list.iter().any(|source| source == "'unsafe-eval'"))
        })
    }
}

/// A violation's blockedURI for a network URL: Chrome 156 names http(s) and ws(s) URLs in
/// full, cross-origin ones included, without fragment or credentials. `None` for other
/// schemes, whose report form the caller keeps.
pub fn network_blocked_uri(target: &str) -> Option<String> {
    let mut url = url::Url::parse(target).ok()?;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") {
        return None;
    }
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Some(url.into())
}

#[derive(Clone, Copy, PartialEq)]
enum SchemeMatch {
    No,
    Exact,
    /// http to https or ws to wss: only through a wildcard, default or 80-to-443 port.
    Upgrade,
}

/// Chrome 156's source-scheme rule (csp-directives control): equal schemes or the
/// secure upgrades http->https and ws->wss. An http(s) source never admits ws(s): only
/// 'self' and `*` cross between them.
fn scheme_match(source: &str, target: &str) -> SchemeMatch {
    if source.eq_ignore_ascii_case(target) {
        return SchemeMatch::Exact;
    }
    match (source.to_ascii_lowercase().as_str(), target) {
        ("http", "https") | ("ws", "wss") => SchemeMatch::Upgrade,
        _ => SchemeMatch::No,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum PortMatch {
    No,
    Exact,
    Wildcard,
    Upgrade,
}

fn port_match(port: Option<&str>, target: &url::Url) -> PortMatch {
    let effective = target.port_or_known_default();
    match port {
        Some("*") => PortMatch::Wildcard,
        None if target.port().is_none() => PortMatch::Wildcard,
        None => PortMatch::No,
        Some(port) => match port.parse::<u16>().ok() {
            Some(port) if Some(port) == effective => PortMatch::Exact,
            Some(80) if effective == Some(443) => PortMatch::Upgrade,
            _ => PortMatch::No,
        },
    }
}

/// CSP3 'self': the document's origin, or its host and port over https/wss (and http/ws
/// from an http document).
fn matches_self(document: &str, target: &str) -> bool {
    if same_origin(document, target) {
        return true;
    }
    let (Ok(origin), Ok(target)) = (url::Url::parse(document), url::Url::parse(target)) else {
        return false;
    };
    origin.host_str().is_some()
        && origin.host_str() == target.host_str()
        && (origin.port_or_known_default() == target.port_or_known_default()
            || origin.port().is_none() && target.port().is_none())
        && (matches!(target.scheme(), "https" | "wss")
            || origin.scheme() == "http" && matches!(target.scheme(), "http" | "ws"))
}

fn matches_host(host: &str, target: &url::Url) -> bool {
    let target_host = target.host_str().unwrap_or("");
    host == "*"
        || host.eq_ignore_ascii_case(target_host)
        || host.strip_prefix("*.").is_some_and(|suffix| {
            target_host
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", suffix.to_ascii_lowercase()))
        })
}

fn matches_path(path: Option<&str>, target: &url::Url, redirected: bool) -> bool {
    redirected
        || path.is_none_or(|path| {
            if path.ends_with('/') {
                target.path().starts_with(path)
            } else {
                target.path() == path
            }
        })
}

fn matches_host_source(source: &str, document: &str, target: &url::Url, redirected: bool) -> bool {
    let (scheme, host_path) = source
        .split_once("://")
        .map_or((None, source), |(scheme, rest)| (Some(scheme), rest));
    // A schemeless source takes the protected resource's scheme.
    let scheme = scheme
        .or_else(|| document.split_once(':').map(|(scheme, _)| scheme))
        .unwrap_or("https");
    let (host_port, path) = host_path.find('/').map_or((host_path, None), |at| {
        (&host_path[..at], Some(&host_path[at..]))
    });
    let (host, port) = host_port
        .rsplit_once(':')
        .map_or((host_port, None), |(host, port)| (host, Some(port)));
    let scheme = scheme_match(scheme, target.scheme());
    let port = port_match(port, target);
    scheme != SchemeMatch::No
        && port != PortMatch::No
        && !(scheme == SchemeMatch::Upgrade && port == PortMatch::Exact)
        && matches_host(host, target)
        && matches_path(path, target, redirected)
}

fn matches_source(source: &str, document: &str, target: &str, redirected: bool) -> bool {
    if source == "'self'" {
        return matches_self(document, target);
    }
    let Ok(url) = url::Url::parse(target) else {
        return false;
    };
    if source == "*" {
        // Chrome 156: `*` admits ws/wss as well as http/https.
        return matches!(url.scheme(), "http" | "https" | "ws" | "wss");
    }
    if let Some(scheme) = source.strip_suffix(':') {
        return scheme_match(scheme, url.scheme()) != SchemeMatch::No;
    }
    !source.starts_with('\'') && matches_host_source(source, document, &url, redirected)
}

pub struct DocumentPolicy {
    pub header: Policy,
    pub active: Policy,
    pub report_only: Policy,
    pub reporting_endpoints: BTreeMap<String, crate::csp_reporting::Endpoint>,
    pub initial_base: Policy,
    pub referrer_policy: turbo_surf_core::referrer_policy::ReferrerPolicy,
    pub embedder_policy: crate::resource_policy::EmbedderPolicy,
    pub permissions_policy: crate::permissions_policy::PermissionsPolicy,
    metas: Vec<(usize, String)>,
    next_meta: usize,
    referrer_metas: Vec<(usize, turbo_surf_core::referrer_policy::ReferrerPolicy)>,
    next_referrer_meta: usize,
    /// Element scripts checked but not yet fetched: the fetch-time report-only check
    /// needs who inserted them ('strict-dynamic').
    element_scripts: Vec<(String, ScriptInsertion)>,
}

type Tree = turbo_dom_parser::rtdom::Tree;
type Handle = turbo_dom_parser::rtdom::Handle;

fn header_values<'a>(
    headers: &'a [(String, String)],
    wanted: &'a str,
) -> impl Iterator<Item = &'a str> + 'a {
    headers
        .iter()
        .filter(move |(name, _)| name.eq_ignore_ascii_case(wanted))
        .map(|(_, value)| value.as_str())
}

fn is_csp_meta(tree: &Tree, node: Handle) -> bool {
    tree.get_attribute(node, "http-equiv")
        .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"))
}

/// Whether a `<head>` (only an HTML one when `html`) is among `node`'s ancestors.
fn in_head(tree: &Tree, node: Handle, html: bool) -> bool {
    let mut parent = tree.parent(node);
    while let Some(handle) = parent {
        let head = if html {
            tree.namespace_id(handle) == 0 && tree.local_name(handle) == Some("head")
        } else {
            tree.tag_name(handle)
                .is_some_and(|tag| tag.eq_ignore_ascii_case("head"))
        };
        if head {
            return true;
        }
        parent = tree.parent(handle);
    }
    false
}

type ReferrerMetas = Vec<(usize, turbo_surf_core::referrer_policy::ReferrerPolicy)>;

/// The head CSP metas and the referrer metas, each keyed by the number of parser
/// scripts before it.
fn parser_metas(tree: &Tree) -> (Vec<(usize, String)>, ReferrerMetas) {
    let (mut index, mut metas, mut referrer_metas) = (0, Vec::new(), Vec::new());
    // The active native script list excludes associated template trees:
    // query_selector_all walks the document's light tree only, so template contents
    // (and their scripts and metas) never appear here
    // (metas_inside_templates_never_activate).
    for &node in tree.query_selector_all("script,meta").iter() {
        if tree
            .tag_name(node)
            .is_some_and(|name| name.eq_ignore_ascii_case("script"))
        {
            index += 1;
            continue;
        }
        let referrer = tree.namespace_id(node) == 0
            && tree
                .get_attribute(node, "name")
                .is_some_and(|name| name.eq_ignore_ascii_case("referrer"));
        if let Some(policy) = referrer
            .then(|| tree.get_attribute(node, "content"))
            .flatten()
            .and_then(crate::referrer_meta::policy)
        {
            referrer_metas.push((index, policy));
        }
        if is_csp_meta(tree, node) && in_head(tree, node, false) {
            if let Some(value) = tree.get_attribute(node, "content") {
                metas.push((index, value.to_string()));
            }
        }
    }
    (metas, referrer_metas)
}

/// `active` plus the head CSP metas before the first `<base href>`: the policy that
/// governs the document's initial base URL.
fn initial_base(tree: &Tree, active: &Policy) -> Policy {
    let mut initial_base = active.clone();
    for &node in tree.query_selector_all("base,meta").iter() {
        if tree.namespace_id(node) != 0 {
            continue;
        }
        if tree.local_name(node) == Some("base") && tree.get_attribute(node, "href").is_some() {
            break;
        }
        if tree.local_name(node) == Some("meta")
            && is_csp_meta(tree, node)
            && in_head(tree, node, true)
        {
            if let Some(value) = tree.get_attribute(node, "content") {
                initial_base.add(value);
            }
        }
    }
    initial_base
}

impl DocumentPolicy {
    pub fn new(base: &str, headers: &[(String, String)], html: &str) -> Self {
        let owned = |name| header_values(headers, name).map(str::to_owned);
        let active = Policy::new(base, owned("content-security-policy"));
        let report_only = Policy::new(base, owned("content-security-policy-report-only"));
        let tree = Tree::parse(html);
        let (metas, referrer_metas) = parser_metas(&tree);
        let initial_base = initial_base(&tree, &active);
        let referrer_header = header_values(headers, "referrer-policy")
            .collect::<Vec<_>>()
            .join(",");
        Self {
            reporting_endpoints: crate::csp_reporting::endpoints(base, headers),
            referrer_policy: turbo_surf_core::referrer_policy::ReferrerPolicy::from_header(
                &referrer_header,
            )
            .unwrap_or_default(),
            embedder_policy: crate::resource_policy::EmbedderPolicy::from_headers(headers),
            permissions_policy: crate::permissions_policy::PermissionsPolicy::from_headers(
                &url::Url::parse(base)
                    .map(|url| url.origin().ascii_serialization())
                    .unwrap_or_else(|_| "null".into()),
                headers,
            ),
            header: active.clone(),
            initial_base,
            active,
            report_only,
            metas,
            next_meta: 0,
            referrer_metas,
            next_referrer_meta: 0,
            element_scripts: Vec::new(),
        }
    }
    /// Records an external `<script>` element's insertion between its element check and
    /// its fetch (bounded: the oldest unfetched note is dropped past 256).
    pub fn note_element_script(&mut self, url: &str, insertion: ScriptInsertion) {
        if self.element_scripts.len() >= 256 {
            self.element_scripts.remove(0);
        }
        self.element_scripts.push((url.to_owned(), insertion));
    }
    /// Who inserted the script a `script-src-elem` request for `url` fetches: a noted
    /// element's insertion, else script-inserted (a module graph import).
    pub fn take_element_script(&mut self, kind: &str, url: &str) -> ScriptInsertion {
        let noted = (kind == "script-src-elem")
            .then(|| {
                self.element_scripts
                    .iter()
                    .position(|(noted, _)| noted == url)
            })
            .flatten();
        noted.map_or(ScriptInsertion::Script, |at| {
            self.element_scripts.remove(at).1
        })
    }
    pub fn unsupported(&self) -> Vec<String> {
        let mut limitations = std::collections::BTreeSet::from_iter(self.active.unsupported());
        limitations.extend(self.report_only.unsupported());
        for (_, value) in &self.metas {
            limitations.extend(Policy::new(&self.header.origin, [value.clone()]).unsupported());
        }
        limitations.into_iter().collect()
    }

    pub fn current(&self) -> &Policy {
        &self.active
    }

    pub fn activate(&mut self, index: usize) {
        while self
            .metas
            .get(self.next_meta)
            .is_some_and(|(before, _)| *before <= index)
        {
            self.active.add(&self.metas[self.next_meta].1);
            self.next_meta += 1;
        }
        while self
            .referrer_metas
            .get(self.next_referrer_meta)
            .is_some_and(|(before, _)| *before <= index)
        {
            self.referrer_policy = self.referrer_metas[self.next_referrer_meta].1;
            self.next_referrer_meta += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn form_action_gates_frame_navigation_hops_and_records_the_first_refusal() {
        let submitter = Policy::new("http://a.test/form", ["form-action 'self'".into()]);
        let gate = Arc::new(FormActionGate::new(submitter));
        let parent = Policy::new("http://a.test/", ["frame-src 'self'".into()]);
        let navigation = parent.with_form_action(gate.clone());
        assert!(navigation.allows_request("frame-src", "http://a.test/next", "", false));
        assert!(navigation.allows_request("img-src", "http://b.test/x", "", true));
        assert_eq!(gate.refused(), None);
        assert!(!navigation.allows_request("frame-src", "http://b.test/hop", "", true));
        assert!(!navigation.allows_request("frame-src", "http://c.test/hop", "", true));
        assert_eq!(gate.refused().as_deref(), Some("http://b.test/hop"));
        assert!(gate.policy().has_policy());
        // The parent's own frame-src still applies, and the parent itself is untouched.
        let open = Arc::new(FormActionGate::new(Policy::default()));
        let gated = parent.with_form_action(open);
        assert!(!gated.allows_request("frame-src", "http://b.test/", "", false));
        assert!(parent.allows_request("frame-src", "http://a.test/", "", false));
    }
    #[test]
    fn implemented_trusted_types_and_sampling_are_not_reported_as_missing() {
        let policy = Policy::new("https://policy.test/", [
            "trusted-types named; require-trusted-types-for 'script'; script-src 'trusted-types-eval' 'report-sample'; style-src 'report-sample'".into(),
        ]);
        let expected = if cfg!(turbo_surf_native_proxy) {
            Vec::<String>::new()
        } else {
            vec![
                "require-trusted-types-for".into(),
                "script-src: 'trusted-types-eval'".into(),
            ]
        };
        assert_eq!(policy.unsupported(), expected);
        let partial = Policy::new(
            "https://policy.test/",
            ["sandbox allow-forms; form-action 'self'; script-src 'strict-dynamic'".into()],
        );
        assert_eq!(partial.unsupported(), vec!["sandbox"]);
        // Dedicated workers execute only as a shell (#104), but `new Worker` enforces
        // worker-src on construction via `allows_request`; only media-src stays missing
        // until media loads land (tracked in #190).
        let missing = Policy::new(
            "https://policy.test/",
            ["worker-src 'none'; media-src 'none'".into()],
        );
        assert_eq!(missing.unsupported(), vec!["media-src"]);
    }

    #[test]
    fn ws_sources_match_like_chrome_156() {
        // harness/detection/fixtures/csp-directives-chrome156.json on an http document.
        let doc = "http://127.0.0.1:4000/?case=0";
        let matches = |source, target| matches_source(source, doc, target, false);
        for target in ["ws://127.0.0.1:4000/x", "wss://127.0.0.1:4000/x"] {
            assert!(matches("'self'", target));
            assert!(matches("*", target));
            assert!(matches("ws:", target));
            assert!(!matches("http:", target));
            assert!(!matches("https:", target));
            assert!(!matches("http://127.0.0.1:4000", target));
            assert!(!matches("127.0.0.1:4000", target));
            assert!(!matches("localhost:*", target));
        }
        assert!(!matches("'self'", "ws://localhost:4000/x"));
        assert!(!matches("wss:", "ws://127.0.0.1:4000/x"));
        assert!(matches("wss:", "wss://127.0.0.1:4000/x"));
        assert!(matches("ws://127.0.0.1:4000", "ws://127.0.0.1:4000/x"));
        // A scheme upgrade needs a wildcard, default or 80->443 port, not an exact one.
        assert!(!matches("ws://127.0.0.1:4000", "wss://127.0.0.1:4000/x"));
        assert!(matches("ws://a.test", "wss://a.test/x"));
        assert!(matches("ws://a.test:*", "wss://a.test:9/x"));
        assert!(matches("http://a.test:80", "https://a.test/x"));
        assert!(!matches("http://a.test:8080", "https://a.test:8080/x"));
        // 'self' admits the https/wss upgrade of an http document's host and port.
        assert!(matches_self("http://a.test/", "https://a.test/x"));
        assert!(!matches_self("https://a.test/", "http://a.test/x"));
        assert!(!matches_self("https://a.test/", "ws://a.test/x"));
        assert!(!matches_self("not a url", "https://a.test/x"));
        assert!(!matches_self("http://a.test:81/", "https://a.test/x"));
    }

    #[test]
    fn blocked_uris_name_network_urls_in_full() {
        assert_eq!(
            network_blocked_uri("http://u:p@x.test:8/a?b#c").as_deref(),
            Some("http://x.test:8/a?b")
        );
        assert_eq!(
            network_blocked_uri("wss://x.test/s#f").as_deref(),
            Some("wss://x.test/s")
        );
        assert_eq!(network_blocked_uri("data:text/plain,x"), None);
        assert_eq!(network_blocked_uri("not a url"), None);
    }

    #[test]
    fn strict_dynamic_trusts_only_script_inserted_scripts() {
        let policy = Policy::new(
            "https://page.test/",
            ["script-src 'nonce-n' 'strict-dynamic' 'self' 'unsafe-inline' https:".into()],
        );
        let script = |url, nonce, insertion| Script {
            url,
            text: "run()",
            nonce,
            insertion,
        };
        let parser = ScriptInsertion::Parser;
        let dynamic = ScriptInsertion::Script;
        assert!(!policy.allows_script(&script("https://page.test/a.js", "", parser)));
        assert!(policy.allows_script(&script("https://other.test/a.js", "n", parser)));
        assert!(policy.allows_script(&script("https://other.test/a.js", "", dynamic)));
        assert!(!policy.allows_script(&script("", "", parser)));
        assert!(policy.allows_script(&script("", "n", parser)));
        assert!(policy.allows_script(&script("", "", dynamic)));
        assert!(!policy.allows_inline("script-src-attr", "run()", ""));
        assert_eq!(
            policy
                .script_violations(&script("https://page.test/a.js", "", parser))
                .len(),
            1
        );
        assert!(policy
            .script_violations(&script("", "", dynamic))
            .is_empty());
        // Module imports and other fetch-stage checks are script-inserted.
        assert!(policy.allows_request("script-src-elem", "https://x.test/m.js", "", false));
        // Non-script directives ignore it and keep their host sources.
        let default = Policy::new(
            "https://page.test/",
            ["default-src 'strict-dynamic' 'self'".into()],
        );
        assert!(default.allows_request("img-src", "https://page.test/i.png", "", false));
        assert!(!default.allows_request("img-src", "https://x.test/i.png", "", false));
    }

    #[test]
    fn worker_and_media_directives_follow_the_csp3_fallback_chain() {
        let allows = |header: &str, kind, target| {
            Policy::new("https://page.test/", [header.to_owned()])
                .allows_request(kind, target, "", false)
        };
        let other = "https://other.test/w.js";
        assert!(!allows(
            "worker-src 'self'; script-src *",
            "worker-src",
            other
        ));
        assert!(!allows(
            "child-src 'self'; script-src *",
            "worker-src",
            other
        ));
        assert!(!allows(
            "script-src 'self'; default-src *",
            "worker-src",
            other
        ));
        assert!(!allows("default-src 'self'", "worker-src", other));
        assert!(allows(
            "worker-src *; child-src 'none'",
            "worker-src",
            other
        ));
        assert!(allows("script-src 'none'", "media-src", other));
        assert!(!allows("default-src 'self'", "media-src", other));
        assert!(allows(
            "media-src *; default-src 'none'",
            "media-src",
            other
        ));
    }

    #[test]
    fn element_script_notes_carry_insertion_to_the_fetch_stage() {
        let mut document = DocumentPolicy::new("https://page.test/", &[], "");
        document.note_element_script("https://page.test/a.js", ScriptInsertion::Parser);
        assert_eq!(
            document.take_element_script("connect-src", "https://page.test/a.js"),
            ScriptInsertion::Script
        );
        assert_eq!(
            document.take_element_script("script-src-elem", "https://page.test/a.js"),
            ScriptInsertion::Parser
        );
        assert_eq!(
            document.take_element_script("script-src-elem", "https://page.test/a.js"),
            ScriptInsertion::Script
        );
        for index in 0..300 {
            document.note_element_script(
                &format!("https://page.test/{index}"),
                ScriptInsertion::Parser,
            );
        }
        assert_eq!(
            document.take_element_script("script-src-elem", "https://page.test/0"),
            ScriptInsertion::Script
        );
        assert_eq!(
            document.take_element_script("script-src-elem", "https://page.test/299"),
            ScriptInsertion::Parser
        );
    }

    #[test]
    fn host_sources_match_schemes_with_the_http_to_https_upgrade_only() {
        // CSP3 6.7.2.8: a host-source's scheme must equal the URL's, except that an
        // "http" source also matches "https"; a schemeless source takes the
        // protected resource's scheme.
        let doc = "https://page.test/";
        let matches = |source, target| matches_source(source, doc, target, false);
        assert!(matches("http://a.test", "https://a.test/x.js"));
        assert!(matches("http://a.test", "http://a.test/x.js"));
        assert!(!matches("https://a.test", "http://a.test/x.js"));
        assert!(!matches("http://a.test", "ws://a.test/x"));
        assert!(matches("a.test", "https://a.test/x.js"));
        assert!(!matches("a.test", "http://a.test/x.js"));
        assert!(matches_source(
            "a.test",
            "http://page.test/",
            "https://a.test/",
            false
        ));
    }

    #[test]
    fn top_navigation_without_activation_sandbox_tokens_intersect() {
        let parse = |header: &str| Policy::new("https://navigation.test/", [header.to_owned()]);
        assert!(!parse("default-src 'self'").blocks_top_navigation_without_activation());
        assert!(parse("sandbox allow-scripts").blocks_top_navigation_without_activation());
        assert!(parse("sandbox allow-top-navigation-by-user-activation")
            .blocks_top_navigation_without_activation());
        assert!(!parse("sandbox ALLOW-TOP-NAVIGATION").blocks_top_navigation_without_activation());
        assert!(parse("sandbox allow-top-navigation, sandbox allow-scripts")
            .blocks_top_navigation_without_activation());
    }

    #[test]
    fn storage_access_sandbox_tokens_intersect() {
        let parse = |header: &str| Policy::new("https://storage.test/", [header.to_owned()]);
        assert!(parse("sandbox allow-scripts allow-same-origin").blocks_storage_access());
        assert!(!parse(
            "sandbox allow-scripts allow-same-origin allow-storage-access-by-user-activation"
        )
        .blocks_storage_access());
        assert!(
            parse("sandbox allow-storage-access-by-user-activation, sandbox allow-scripts")
                .blocks_storage_access()
        );
    }

    #[test]
    fn policy_snapshot_keeps_reporting_provenance_after_meta_and_header_changes() {
        let mut active = Policy::new(
            "https://source.test/page",
            ["script-src 'none'; report-uri /initial".into()],
        );
        let initial = active.clone();
        active.add("script-src 'none'; report-uri /ignored-meta");
        let before_inheritance = active.clone();
        active.inherit_headers(&Policy::new(
            "https://source.test/page",
            ["script-src 'none'; report-to new-group".into()],
        ));
        let rows = initial.inline_violations("script-src-elem", "blocked()", "");
        assert_eq!(rows.len(), 1);
        assert!(
            matches!(&rows[0].destination, ReportDestination::Legacy(urls) if urls == &["/initial"])
        );
        let rows = before_inheritance.inline_violations("script-src-elem", "blocked()", "");
        assert_eq!(rows.len(), 2);
        assert!(
            matches!(&rows[0].destination, ReportDestination::Legacy(urls) if urls == &["/initial"])
        );
        assert!(matches!(&rows[1].destination, ReportDestination::None));
        let rows = active.inline_violations("script-src-elem", "blocked()", "");
        assert_eq!(rows.len(), 3);
        assert!(
            matches!(&rows[0].destination, ReportDestination::Group(group) if group == "new-group")
        );
        assert!(
            matches!(&rows[1].destination, ReportDestination::Legacy(urls) if urls == &["/initial"])
        );
        assert!(matches!(&rows[2].destination, ReportDestination::None));
    }
    #[test]
    fn policies_intersect_sources_and_real_hashes() {
        let text = "globalThis.hashAllowed=true;";
        let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(text));
        let p=Policy::new("https://source.test/page",[format!("script-src 'nonce-good' 'sha256-{hash}' 'self'; img-src https://*.images.test:*/path/")]);
        assert!(p.allows_inline("script-src-elem", text, ""));
        assert!(!p.allows_inline("script-src-elem", "other", ""));
        assert!(p.allows_inline("script-src-elem", "other", "good"));
        assert!(!p.allows_inline("script-src-attr", text, "good"));
        assert!(p.allows_request("script-src-elem", "https://source.test/a", "", false));
        assert!(!p.allows_request("script-src-elem", "https://cross.test/a", "", false));
        assert!(p.allows_request("img-src", "https://sub.images.test:8040/path/a", "", false));
        assert!(!p.allows_request("img-src", "https://images.test/path/a", "", false));
        assert!(!p.allows_eval());
        let p = Policy::new(
            "https://source.test",
            ["script-src 'self'".into(), "default-src 'none'".into()],
        );
        assert!(!p.allows_request("script-src-elem", "https://source.test/a", "", false));
    }
    #[test]
    fn meta_policy_does_not_retroactively_block_earlier_scripts() {
        let mut p=DocumentPolicy::new("https://source.test",&[],"<head><script>before()</script><meta http-equiv='Content-Security-Policy' content=\"script-src 'none'\"><script>after()</script></head>");
        p.activate(0);
        assert!(p.active.allows_inline("script-src-elem", "before()", ""));
        p.activate(1);
        assert!(!p.active.allows_inline("script-src-elem", "after()", ""));
    }
    #[test]
    fn base_and_form_directives_do_not_fall_back_to_default_sources() {
        let policy = Policy::new("https://source.test/root", ["default-src 'none'".into()]);
        assert!(policy.allows_request("base-uri", "https://cross.test/root", "", false));
        assert!(policy.allows_request("form-action", "https://cross.test/send", "", false));
        let policy = Policy::new(
            "https://source.test/root",
            ["default-src *;base-uri 'self';form-action 'none'".into()],
        );
        assert!(policy.allows_request("base-uri", "https://source.test/root", "", false));
        assert!(!policy.allows_request("base-uri", "https://cross.test/root", "", false));
        assert!(!policy.allows_request("form-action", "https://source.test/send", "", false));
    }
    #[test]
    fn initial_base_freezes_before_later_metas_and_intersects_inherited_headers() {
        let html = r#"<head><base href='/allowed/'><meta http-equiv='Content-Security-Policy' content="base-uri 'none'"></head>"#;
        let mut policy = DocumentPolicy::new("https://source.test/root", &[], html);
        assert!(policy.initial_base.allows_request(
            "base-uri",
            "https://source.test/allowed/",
            "",
            false
        ));
        policy.activate(0);
        assert!(!policy.current().allows_request(
            "base-uri",
            "https://source.test/allowed/",
            "",
            false
        ));
        let html = r#"<head><meta http-equiv='Content-Security-Policy' content="base-uri 'none'"><base href='/blocked/'></head>"#;
        let mut policy = DocumentPolicy::new("about:blank", &[], html);
        policy.initial_base.inherit_headers(&Policy::new(
            "https://source.test",
            ["base-uri 'self'".into()],
        ));
        assert!(!policy.initial_base.allows_request(
            "base-uri",
            "https://source.test/blocked/",
            "",
            false
        ));
    }

    #[test]
    fn source_expressions_match_ports_keywords_and_bad_urls_like_chrome() {
        let doc = "https://page.test/";
        // A keyword never matches a URL; an unparsable target never matches.
        assert!(!matches_source(
            "'unsafe-inline'",
            doc,
            "https://x.test/",
            false
        ));
        assert!(!matches_source("https:", doc, "not a url", false));
        // Explicit port: exact or the scheme default; `*` any; none = default only.
        assert!(matches_source(
            "x.test:8080",
            doc,
            "https://x.test:8080/a",
            false
        ));
        assert!(!matches_source(
            "x.test:8080",
            doc,
            "https://x.test:8081/a",
            false
        ));
        assert!(matches_source("x.test:443", doc, "https://x.test/a", false));
        assert!(!matches_source(
            "x.test:abc",
            doc,
            "https://x.test/a",
            false
        ));
        assert!(matches_source("x.test:*", doc, "https://x.test:9/a", false));
        assert!(matches_source("x.test", doc, "https://x.test:443/a", false));
        assert!(!matches_source(
            "x.test",
            doc,
            "https://x.test:8443/a",
            false
        ));
    }

    #[test]
    fn eval_violations_report_each_policy_without_unsafe_eval() {
        let policy = Policy::new(
            "https://eval.test/",
            [
                "script-src 'self' 'report-sample'; report-uri /r".into(),
                "script-src 'unsafe-eval'".into(),
                "img-src 'none'".into(),
            ],
        );
        let records = policy.eval_violations();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].original,
            "script-src 'self' 'report-sample'; report-uri /r"
        );
        assert!(records[0].sample);
        assert!(matches!(
            &records[0].destination,
            ReportDestination::Legacy(uris) if uris == &["/r".to_string()]
        ));
        let quiet = Policy::new("https://eval.test/", ["script-src 'self'".into()]);
        assert!(!quiet.eval_violations()[0].sample);
        assert!(
            Policy::new("https://eval.test/", ["script-src 'unsafe-eval'".into()])
                .eval_violations()
                .is_empty()
        );
    }

    #[test]
    fn metas_inside_templates_never_activate() {
        let html = "<head><template><meta http-equiv=Content-Security-Policy \
                    content=\"img-src 'none'\"><script></script></template>\
                    <meta http-equiv=Content-Security-Policy content=\"script-src 'self'\"></head>";
        let mut doc = DocumentPolicy::new("https://meta.test/", &[], html);
        doc.activate(usize::MAX);
        assert!(doc
            .current()
            .allows_request("img-src", "https://other.test/i.png", "", false));
        assert!(!doc.current().allows_request(
            "script-src-elem",
            "https://other.test/s.js",
            "",
            false
        ));
        // A template's <base> does not end the initial-base meta scan either.
        let html = "<head><template><base href='/t/'></template>\
                    <meta http-equiv=Content-Security-Policy content=\"base-uri 'none'\"></head>";
        let doc = DocumentPolicy::new("https://meta.test/", &[], html);
        assert!(!doc
            .initial_base
            .allows_request("base-uri", "https://meta.test/t/", "", false));
    }
}
