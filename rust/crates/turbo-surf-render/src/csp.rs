//! Enforced fetch and inline-script CSP subset; unsupported directives are not
//! represented as implemented. Header policies intersect, as do encountered meta policies.
use base64::Engine;
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::{collections::BTreeMap, sync::Arc};
use turbo_surf_core::request_context::same_origin;

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
    fn list<'a>(policy: &'a BTreeMap<String, Vec<String>>, kind: &str) -> Option<&'a [String]> {
        if matches!(kind, "base-uri" | "form-action") {
            return policy.get(kind).map(Vec::as_slice);
        }
        let fallback = match kind {
            "script-src-elem" | "script-src-attr" => Some("script-src"),
            "style-src-elem" | "style-src-attr" => Some("style-src"),
            "frame-src" => Some("child-src"),
            _ => None,
        };
        policy
            .get(kind)
            .or_else(|| fallback.and_then(|name| policy.get(name)))
            .or_else(|| policy.get("default-src"))
            .map(Vec::as_slice)
    }
    pub fn allows_request(&self, kind: &str, target: &str, nonce: &str, redirected: bool) -> bool {
        self.policies.iter().all(|policy| {
            Self::list(policy, kind).is_none_or(|list| {
                (!nonce.is_empty()
                    && matches!(kind, "script-src-elem" | "style-src-elem")
                    && list
                        .iter()
                        .any(|source| source == &format!("'nonce-{nonce}'")))
                    || list
                        .iter()
                        .any(|source| matches_source(source, &self.origin, target, redirected))
            })
        })
    }
    pub fn request_violations(&self, kind: &str, url: &str, nonce: &str) -> Vec<ReportRecord> {
        self.policies
            .iter()
            .zip(self.originals.iter())
            .zip(self.header_reports.iter())
            .filter_map(|((policy, original), header)| {
                let single = Self {
                    origin: self.origin.clone(),
                    policies: Arc::new(vec![policy.clone()]),
                    originals: Arc::new(vec![original.clone()]),
                    header_reports: Arc::new(vec![*header]),
                };
                (!single.allows_request(kind, url, nonce, false))
                    .then(|| Self::report_record(policy, original, false, *header))
            })
            .collect()
    }
    pub fn inline_violations(&self, kind: &str, text: &str, nonce: &str) -> Vec<ReportRecord> {
        self.policies
            .iter()
            .zip(self.originals.iter())
            .zip(self.header_reports.iter())
            .filter_map(|((policy, original), header)| {
                let single = Self {
                    origin: self.origin.clone(),
                    policies: Arc::new(vec![policy.clone()]),
                    originals: Arc::new(vec![original.clone()]),
                    header_reports: Arc::new(vec![*header]),
                };
                (!single.allows_inline(kind, text, nonce)).then(|| {
                    let sample = Self::list(policy, kind)
                        .is_some_and(|list| list.iter().any(|token| token == "'report-sample'"));
                    Self::report_record(policy, original, sample, *header)
                })
            })
            .collect()
    }
    pub fn eval_violations(&self) -> Vec<ReportRecord> {
        self.policies
            .iter()
            .zip(self.originals.iter())
            .zip(self.header_reports.iter())
            .filter_map(|((policy, original), header)| {
                let list = Self::list(policy, "script-src")?;
                (!list.iter().any(|token| token == "'unsafe-eval'")).then(|| {
                    Self::report_record(
                        policy,
                        original,
                        list.iter().any(|token| token == "'report-sample'"),
                        *header,
                    )
                })
            })
            .collect()
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
    pub fn allows_inline(&self, kind: &str, text: &str, nonce: &str) -> bool {
        let hash = |algorithm: &str, bytes: Vec<u8>| {
            format!(
                "'{algorithm}-{}'",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        };
        let hashes = [
            hash("sha256", Sha256::digest(text.as_bytes()).to_vec()),
            hash("sha384", Sha384::digest(text.as_bytes()).to_vec()),
            hash("sha512", Sha512::digest(text.as_bytes()).to_vec()),
        ];
        self.policies.iter().all(|policy| {
            Self::list(policy, kind).is_none_or(|list| {
                let protected = list.iter().any(|source| {
                    source.starts_with("'nonce-")
                        || source.starts_with("'sha256-")
                        || source.starts_with("'sha384-")
                        || source.starts_with("'sha512-")
                });
                let attribute = kind.ends_with("-attr");
                (!attribute
                    && !nonce.is_empty()
                    && list
                        .iter()
                        .any(|source| source == &format!("'nonce-{nonce}'")))
                    || ((!attribute || list.iter().any(|source| source == "'unsafe-hashes'"))
                        && list.iter().any(|source| hashes.contains(source)))
                    || (!protected && list.iter().any(|source| source == "'unsafe-inline'"))
            })
        })
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
                        "'strict-dynamic'" | "'wasm-unsafe-eval'" | "'inline-speculation-rules'"
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
        self.policies
            .iter()
            .zip(self.originals.iter())
            .zip(self.header_reports.iter())
            .filter_map(|((policy, original), header)| {
                let violation = if let Some((name, duplicate)) = name {
                    policy.get("trusted-types").is_some_and(|tokens| {
                        !tokens.iter().any(|token| token == name || token == "*")
                            || duplicate
                                && (name == "default"
                                    || !tokens.iter().any(|token| token == "'allow-duplicates'"))
                    })
                } else {
                    policy
                        .get("require-trusted-types-for")
                        .is_some_and(|tokens| tokens.iter().any(|token| token == "'script'"))
                };
                violation.then(|| Self::report_record(policy, original, true, *header))
            })
            .collect()
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

fn matches_source(source: &str, document: &str, target: &str, redirected: bool) -> bool {
    if source == "'self'" {
        return same_origin(document, target);
    }
    let Ok(target) = url::Url::parse(target) else {
        return false;
    };
    if source == "*" {
        return matches!(target.scheme(), "http" | "https");
    }
    if let Some(scheme) = source.strip_suffix(':') {
        return target.scheme() == scheme || scheme == "http" && target.scheme() == "https";
    }
    if source.starts_with('\'') {
        return false;
    }
    let (scheme, host_path) = source
        .split_once("://")
        .map_or((None, source), |(scheme, rest)| (Some(scheme), rest));
    let scheme = scheme
        .or_else(|| document.split_once(':').map(|(scheme, _)| scheme))
        .unwrap_or("https");
    if target.scheme() != scheme && !(scheme == "http" && target.scheme() == "https") {
        return false;
    }
    let (host_port, path) = host_path
        .split_once('/')
        .map_or((host_path, None), |(host, path)| {
            (host, Some(format!("/{path}")))
        });
    let (host, port) = host_port
        .rsplit_once(':')
        .map_or((host_port, None), |(host, port)| (host, Some(port)));
    let target_host = target.host_str().unwrap_or("");
    let host_matches = host == "*"
        || host.eq_ignore_ascii_case(target_host)
        || host.strip_prefix("*.").is_some_and(|suffix| {
            target_host
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", suffix.to_ascii_lowercase()))
        });
    let port_matches = match port {
        Some("*") => true,
        Some(port) => port.parse::<u16>().ok() == target.port_or_known_default(),
        None => target.port().is_none(),
    };
    host_matches
        && port_matches
        && (redirected
            || path.is_none_or(|path| {
                if path.ends_with('/') {
                    target.path().starts_with(&path)
                } else {
                    target.path() == path
                }
            }))
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
}
impl DocumentPolicy {
    pub fn new(base: &str, headers: &[(String, String)], html: &str) -> Self {
        let values = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-security-policy"))
            .map(|(_, value)| value.clone());
        let active = Policy::new(base, values);
        let report_only = Policy::new(
            base,
            headers
                .iter()
                .filter(|(name, _)| {
                    name.eq_ignore_ascii_case("content-security-policy-report-only")
                })
                .map(|(_, value)| value.clone()),
        );
        let tree = turbo_dom_parser::rtdom::Tree::parse(html);
        let mut index = 0;
        let mut metas = Vec::new();
        let mut referrer_metas = Vec::new();
        for &node in tree.query_selector_all("script,meta").iter() {
            // The active native script list excludes associated template trees:
            // query_selector_all walks the document's light tree only, so template
            // contents (and their scripts and metas) never appear here
            // (metas_inside_templates_never_activate).
            if tree
                .tag_name(node)
                .is_some_and(|name| name.eq_ignore_ascii_case("script"))
            {
                index += 1;
                continue;
            }
            if tree.namespace_id(node) == 0
                && tree
                    .get_attribute(node, "name")
                    .is_some_and(|name| name.eq_ignore_ascii_case("referrer"))
            {
                if let Some(policy) = tree
                    .get_attribute(node, "content")
                    .and_then(crate::referrer_meta::policy)
                {
                    referrer_metas.push((index, policy));
                }
            }
            if !tree
                .get_attribute(node, "http-equiv")
                .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"))
            {
                continue;
            }
            let mut parent = turbo_dom_parser::rtdom::NodeRef::new(&tree, node).parent();
            let mut in_head = false;
            while let Some(node) = parent {
                if tree
                    .tag_name(node.handle())
                    .is_some_and(|tag| tag.eq_ignore_ascii_case("head"))
                {
                    in_head = true;
                    break;
                }
                parent = node.parent();
            }
            if in_head {
                if let Some(value) = tree.get_attribute(node, "content") {
                    metas.push((index, value.to_string()));
                }
            }
        }
        let mut initial_base = active.clone();
        for &node in tree.query_selector_all("base,meta").iter() {
            if tree.namespace_id(node) != 0 {
                continue;
            }
            if tree.local_name(node) == Some("base") && tree.get_attribute(node, "href").is_some() {
                break;
            }
            if tree.local_name(node) != Some("meta")
                || !tree
                    .get_attribute(node, "http-equiv")
                    .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"))
            {
                continue;
            }
            let mut parent = tree.parent(node);
            while let Some(handle) = parent {
                if tree.local_name(handle) == Some("head") && tree.namespace_id(handle) == 0 {
                    if let Some(value) = tree.get_attribute(node, "content") {
                        initial_base.add(value);
                    }
                    break;
                }
                parent = tree.parent(handle);
            }
        }
        let referrer_header = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("referrer-policy"))
            .map(|(_, value)| value.as_str())
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
        }
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
        assert_eq!(
            partial.unsupported(),
            vec!["form-action", "sandbox", "script-src: 'strict-dynamic'"]
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
