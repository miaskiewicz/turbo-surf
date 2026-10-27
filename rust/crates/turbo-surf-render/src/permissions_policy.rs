//! Parsed response policies; author globals never grant a native capability.
use std::collections::{BTreeMap, BTreeSet};

// Measured against the pinned Chrome 155 control. `true` is the default `*`;
// every other known feature defaults to the document origin.
const FEATURES: &[(&str, bool)] = &[
    ("geolocation", false),
    ("ch-ua-full-version-list", false),
    ("cross-origin-isolated", false),
    ("screen-wake-lock", false),
    ("on-device-speech-recognition", false),
    ("translator", false),
    ("publickey-credentials-get", false),
    ("ch-ua-arch", false),
    ("bluetooth", false),
    ("compute-pressure", false),
    ("ch-prefers-reduced-transparency", false),
    ("deferred-fetch", false),
    ("usb", false),
    ("ch-save-data", true),
    ("publickey-credentials-create", false),
    ("deferred-fetch-minimal", true),
    ("ch-downlink", false),
    ("ch-ua-form-factors", false),
    ("otp-credentials", false),
    ("payment", false),
    ("ch-ua", true),
    ("ch-ua-model", false),
    ("ch-ect", false),
    ("autoplay", false),
    ("camera", false),
    ("language-detector", false),
    ("private-state-token-issuance", true),
    ("digital-credentials-get", false),
    ("accelerometer", false),
    ("ch-ua-platform-version", false),
    ("idle-detection", false),
    ("language-model", false),
    ("speaker-selection", false),
    ("interest-cohort", true),
    ("ch-viewport-height", false),
    ("captured-surface-control", false),
    ("local-fonts", false),
    ("ch-ua-platform", true),
    ("midi", false),
    ("ch-ua-full-version", false),
    ("xr-spatial-tracking", false),
    ("clipboard-read", false),
    ("gamepad", true),
    ("display-capture", false),
    ("keyboard-map", false),
    ("aria-notify", true),
    ("local-network", false),
    ("ch-ua-high-entropy-values", true),
    ("ch-width", false),
    ("ch-prefers-reduced-motion", false),
    ("browsing-topics", true),
    ("encrypted-media", false),
    ("local-network-access", false),
    ("gyroscope", false),
    ("serial", false),
    ("ch-rtt", false),
    ("ch-ua-mobile", true),
    ("window-management", false),
    ("unload", true),
    ("ch-dpr", false),
    ("ch-prefers-color-scheme", false),
    ("ch-ua-wow64", false),
    ("fullscreen", false),
    ("identity-credentials-get", false),
    ("private-state-token-redemption", true),
    ("hid", false),
    ("summarizer", false),
    ("ch-ua-bitness", false),
    ("digital-credentials-create", false),
    ("storage-access", true),
    ("sync-xhr", true),
    ("ch-device-memory", false),
    ("ch-viewport-width", false),
    ("picture-in-picture", true),
    ("loopback-network", false),
    ("magnetometer", false),
    ("clipboard-write", false),
    ("media-playback-while-not-visible", true),
    ("microphone", false),
];

#[derive(Clone, Default)]
pub struct PermissionsPolicy {
    declared: BTreeMap<String, Vec<String>>,
    inherited_denials: BTreeSet<String>,
}
impl PermissionsPolicy {
    pub fn from_headers(origin: &str, headers: &[(String, String)]) -> Self {
        let value = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("permissions-policy"))
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let mut policy = Self::default();
        let Ok(dictionary) = sfv::Parser::new(&value)
            .with_version(sfv::Version::Rfc8941)
            .parse::<sfv::Dictionary>()
        else {
            return policy;
        };
        for (name, entry) in dictionary {
            if !FEATURES
                .iter()
                .any(|(feature, _)| *feature == name.as_str())
            {
                continue;
            }
            let items = match entry {
                sfv::ListEntry::InnerList(list) => list.items,
                sfv::ListEntry::Item(item) => {
                    match item.bare_item.as_token().map(|value| value.as_str()) {
                        Some("*") => vec![item],
                        Some(_) => continue,
                        None => vec![item],
                    }
                }
            };
            let mut list = Vec::new();
            for item in items {
                let value = match item.bare_item.as_token().map(|value| value.as_str()) {
                    Some("self") => Some(origin.to_string()),
                    Some("*") => Some("*".to_string()),
                    _ => item.bare_item.as_string().and_then(|value| {
                        if value.as_str() == "*" {
                            Some("*".to_string())
                        } else {
                            parse_allowlist_origin(value.as_str())
                        }
                    }),
                };
                if let Some(value) = value {
                    if !list.contains(&value) {
                        list.push(value);
                    }
                }
            }
            policy.declared.insert(name.as_str().to_string(), list);
        }
        policy
    }
    pub fn inherit(
        &mut self,
        parent: &Self,
        parent_origin: &str,
        child_origin: &str,
        source_origin: &str,
        allow: &str,
    ) {
        let mut container = BTreeMap::new();
        for directive in allow.split(';') {
            let mut words = directive.split_ascii_whitespace();
            let Some(name) = words.next() else {
                continue;
            };
            container
                .entry(name)
                .or_insert_with(|| words.collect::<Vec<_>>());
        }
        for (name, wildcard) in FEATURES {
            let parent_enabled = parent.allows_self(name, parent_origin);
            let declared_allows = parent.declared.get(*name).is_none_or(|list| {
                list.iter()
                    .any(|origin| origin == "*" || matches_origin(origin, child_origin))
            });
            let container_allows = container.get(name).map_or(
                *wildcard || parent_origin != "null" && parent_origin == child_origin,
                |tokens| {
                    if tokens.is_empty() {
                        return source_origin != "null" && source_origin == child_origin;
                    }
                    tokens.iter().any(|token| match *token {
                        "*" => true,
                        "'self'" => parent_origin != "null" && parent_origin == child_origin,
                        "'src'" => source_origin != "null" && source_origin == child_origin,
                        "'none'" => false,
                        origin => matches_origin(origin, child_origin),
                    })
                },
            );
            if !parent_enabled || !declared_allows || !container_allows {
                self.inherited_denials.insert((*name).to_string());
            }
        }
    }
    pub fn features() -> Vec<&'static str> {
        FEATURES.iter().map(|(name, _)| *name).collect()
    }
    pub fn allows_self(&self, name: &str, origin: &str) -> bool {
        if self.inherited_denials.contains(name)
            || !FEATURES.iter().any(|(feature, _)| *feature == name)
        {
            return false;
        }
        if origin != "null" {
            return self.allows(name, origin, origin);
        }
        self.declared
            .get(name)
            .is_none_or(|list| list.iter().any(|source| source == "*" || source == "null"))
    }
    pub fn allowlist(&self, name: &str, origin: &str) -> Vec<String> {
        if self.inherited_denials.contains(name) {
            return Vec::new();
        }
        let Some((_, wildcard)) = FEATURES.iter().find(|(feature, _)| *feature == name) else {
            return Vec::new();
        };
        self.declared
            .get(name)
            .cloned()
            .unwrap_or_else(|| vec![if *wildcard { "*".into() } else { origin.into() }])
            .into_iter()
            .filter(|source| source != "null")
            .collect()
    }
    pub fn allows(&self, name: &str, document_origin: &str, target: &str) -> bool {
        if parse_origin(target).is_none() {
            return false;
        }
        self.allowlist(name, document_origin)
            .iter()
            .any(|allowed| allowed == "*" || matches_origin(allowed, target))
    }
}
fn parse_origin(value: &str) -> Option<String> {
    let origin = url::Url::parse(value).ok()?.origin().ascii_serialization();
    (origin != "null").then_some(origin)
}

fn parse_allowlist_origin(value: &str) -> Option<String> {
    let authority = value.split_once("://")?.1.split(['/', '?', '#']).next()?;
    let port = authority
        .rsplit_once(':')
        .map(|(_, port)| port)
        .filter(|port| *port == "*" || port.bytes().all(|byte| byte.is_ascii_digit()));
    let any_port = port == Some("*");
    let value = if any_port {
        value.replacen(":*", ":65535", 1)
    } else {
        value.to_string()
    };
    let parsed = url::Url::parse(&value).ok()?;
    let origin = parsed.origin().ascii_serialization();
    if origin == "null" {
        return None;
    }
    if any_port {
        return origin
            .strip_suffix(":65535")
            .map(|origin| format!("{origin}:*"));
    }
    if let Some(port) = port {
        if parsed.port().is_none() {
            return Some(format!("{origin}:{}", port.parse::<u16>().ok()?));
        }
    }
    Some(origin)
}
fn matches_origin(pattern: &str, target: &str) -> bool {
    let Some(authority) = pattern
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
    else {
        return false;
    };
    let any_port = authority.ends_with(":*");
    let pattern = if any_port {
        pattern.replacen(":*", ":65535", 1)
    } else {
        pattern.to_string()
    };
    let (Ok(pattern), Ok(target)) = (url::Url::parse(&pattern), url::Url::parse(target)) else {
        return false;
    };
    if pattern.scheme() != target.scheme() {
        return false;
    }
    if !any_port && pattern.port_or_known_default() != target.port_or_known_default() {
        return false;
    }
    let (Some(pattern), Some(target)) = (pattern.host_str(), target.host_str()) else {
        return false;
    };
    if let Some(suffix) = pattern.strip_prefix("*.") {
        target
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1)
    } else {
        pattern == target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(header: &str) -> PermissionsPolicy {
        PermissionsPolicy::from_headers(
            "https://site.test",
            &[("Permissions-Policy".into(), header.into())],
        )
    }

    #[test]
    fn unknown_features_are_ignored_and_a_bare_star_allows_all() {
        let p = policy("made-up=*");
        assert!(p.allowlist("made-up", "https://site.test").is_empty());
        assert!(!p.allows_self("made-up", "https://site.test"));
        assert!(policy("geolocation=*").allows("geolocation", "https://site.test", "https://x.test"));
    }

    #[test]
    #[ignore = "turbo-surf#59: fails until the fix"]
    fn bare_token_allowlists_match_chrome() {
        // Chrome 156: `ch-ua=self` → [self origin]; `ch-save-data=bogus` → [] (disabled),
        // both default-`*` features (turbo-surf#59).
        let p = policy("ch-ua=self, ch-save-data=bogus");
        assert_eq!(
            p.allowlist("ch-ua", "https://site.test"),
            vec!["https://site.test".to_string()]
        );
        assert!(p.allowlist("ch-save-data", "https://site.test").is_empty());
        assert!(!p.allows("ch-ua", "https://site.test", "https://x.test"));
    }

    #[test]
    fn allowlist_origins_match_scheme_port_and_wildcard_hosts() {
        let p = policy(r#"geolocation=("https://*.cdn.test" "https://a.test:8443" "http://b.test:*")"#);
        let allows = |target| p.allows("geolocation", "https://site.test", target);
        assert!(allows("https://img.cdn.test"));
        assert!(!allows("https://cdn.test"));
        assert!(!allows("http://img.cdn.test"));
        assert!(allows("https://a.test:8443"));
        assert!(!allows("https://a.test"));
        assert!(allows("http://b.test:9"));
        assert!(!allows("data:text/plain,x"));
        assert!(!matches_origin("no-scheme", "https://a.test"));
        assert!(!matches_origin("https://a.test", "not a url"));
        assert!(!matches_origin("https://a.test", "file:///x"));
        assert_eq!(parse_allowlist_origin("data://x"), None);
        assert_eq!(
            parse_allowlist_origin("https://a.test:8443/path").as_deref(),
            Some("https://a.test:8443")
        );
        assert_eq!(
            parse_allowlist_origin("https://*.a.test:*").as_deref(),
            Some("https://*.a.test:*")
        );
    }
}
