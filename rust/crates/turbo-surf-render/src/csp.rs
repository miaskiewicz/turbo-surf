//! Enforced fetch and inline-script CSP subset; unsupported directives are not
//! represented as implemented. Header policies intersect, as do encountered meta policies.
use base64::Engine;
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::collections::BTreeMap;
use turbo_surf_core::request_context::same_origin;

#[derive(Clone, Default)]
pub struct Policy {
    origin: String,
    policies: Vec<BTreeMap<String, Vec<String>>>,
}
impl Policy {
    pub fn new(origin: &str, values: impl IntoIterator<Item = String>) -> Self {
        let policies = values
            .into_iter()
            .flat_map(|value| value.split(',').map(str::to_owned).collect::<Vec<_>>())
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
            origin: origin.to_string(),
            policies,
        }
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn add(&mut self, value: &str) {
        self.policies
            .extend(Self::new(&self.origin, [value.to_string()]).policies);
    }
    pub fn inherit_headers(&mut self, inherited: &Self) {
        let own = std::mem::take(&mut self.policies);
        *self = inherited.clone();
        self.policies.extend(own);
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
        ];
        let mut limitations = std::collections::BTreeSet::new();
        for policy in &self.policies {
            for (name, values) in policy {
                if !SUPPORTED.contains(&name.as_str()) {
                    limitations.insert(name.clone());
                }
                for value in values {
                    if matches!(
                        value.as_str(),
                        "'strict-dynamic'"
                            | "'wasm-unsafe-eval'"
                            | "'trusted-types-eval'"
                            | "'inline-speculation-rules'"
                            | "'report-sample'"
                    ) {
                        limitations.insert(format!("{name}: {value}"));
                    }
                }
            }
        }
        limitations.into_iter().collect()
    }

    pub fn has_policy(&self) -> bool {
        !self.policies.is_empty()
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
    pub initial_base: Policy,
    pub referrer_policy: turbo_surf_core::referrer_policy::ReferrerPolicy,
    pub embedder_policy: crate::resource_policy::EmbedderPolicy,
    metas: Vec<(usize, String)>,
    next_meta: usize,
}
impl DocumentPolicy {
    pub fn new(base: &str, headers: &[(String, String)], html: &str) -> Self {
        let values = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-security-policy"))
            .map(|(_, value)| value.clone());
        let active = Policy::new(base, values);
        let tree = turbo_dom_parser::rtdom::Tree::parse(html);
        let mut index = 0;
        let mut metas = Vec::new();
        for &node in tree.query_selector_all("script,meta").iter() {
            if tree
                .tag_name(node)
                .is_some_and(|name| name.eq_ignore_ascii_case("script"))
            {
                index += 1;
                continue;
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
            referrer_policy: turbo_surf_core::referrer_policy::ReferrerPolicy::from_header(
                &referrer_header,
            )
            .unwrap_or_default(),
            embedder_policy: crate::resource_policy::EmbedderPolicy::from_headers(headers),
            header: active.clone(),
            initial_base,
            active,
            metas,
            next_meta: 0,
        }
    }
    pub fn unsupported(&self) -> Vec<String> {
        let mut limitations = std::collections::BTreeSet::from_iter(self.active.unsupported());
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
