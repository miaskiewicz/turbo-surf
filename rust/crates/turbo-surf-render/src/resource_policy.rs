//! Response policy checks run before opaque bytes reach page consumers.
use turbo_surf_core::request_context::{same_origin, same_site, RequestContext, RequestMode};

pub fn active_mixed_content_allowed(source: &str, target: &str) -> bool {
    let (Ok(source), Ok(target)) = (url::Url::parse(source), url::Url::parse(target)) else {
        return false;
    };
    if source.scheme() != "https" || target.scheme() != "http" {
        return true;
    }
    match target.host() {
        Some(url::Host::Domain(host)) => host == "localhost" || host.ends_with(".localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmbedderPolicy {
    #[default]
    UnsafeNone,
    RequireCorp,
    Credentialless,
}

impl EmbedderPolicy {
    pub fn from_headers(headers: &[(String, String)]) -> Self {
        let Some(value) = crate::fetch_cors::header(headers, "cross-origin-embedder-policy") else {
            return Self::UnsafeNone;
        };
        // The fresh Chrome 155 control rejects RFC 9651 date/display-string parameters.
        let Ok(item) = sfv::Parser::new(value)
            .with_version(sfv::Version::Rfc8941)
            .parse::<sfv::Item>()
        else {
            return Self::UnsafeNone;
        };
        match item.bare_item.as_token().map(|token| token.as_str()) {
            Some("require-corp") => Self::RequireCorp,
            Some("credentialless") => Self::Credentialless,
            _ => Self::UnsafeNone,
        }
    }
}

pub fn isolated_opener(headers: &[(String, String)]) -> bool {
    crate::fetch_cors::header(headers, "cross-origin-opener-policy")
        .and_then(|value| {
            sfv::Parser::new(value)
                .with_version(sfv::Version::Rfc8941)
                .parse::<sfv::Item>()
                .ok()
        })
        .is_some_and(|item| {
            item.bare_item
                .as_token()
                .is_some_and(|token| token.as_str() == "same-origin")
        })
}

#[cfg(test)]
pub fn corp_allowed(context: &RequestContext, target: &str, headers: &[(String, String)]) -> bool {
    embedder_allowed(EmbedderPolicy::UnsafeNone, context, target, headers)
}

pub fn embedder_allowed(
    policy: EmbedderPolicy,
    context: &RequestContext,
    target: &str,
    headers: &[(String, String)],
) -> bool {
    if context.mode != RequestMode::NoCors {
        return true;
    }
    let source = context.source.as_deref().unwrap_or("");
    let same_origin = !context.opaque_origin && same_origin(source, target);
    if same_origin {
        return true;
    }
    match crate::fetch_cors::header(headers, "cross-origin-resource-policy") {
        Some("same-origin") => false,
        Some("same-site") => !context.opaque_origin && corp_same_site(source, target),
        Some("cross-origin") => true,
        // Missing, invalid and duplicate CORP fields inherit COEP's default.
        _ => {
            policy == EmbedderPolicy::UnsafeNone
                || policy == EmbedderPolicy::Credentialless && !context.sends_credentials(target)
        }
    }
}

fn corp_same_site(source: &str, target: &str) -> bool {
    let (Ok(source_url), Ok(mut target_url)) = (url::Url::parse(source), url::Url::parse(target))
    else {
        return false;
    };
    if source_url.scheme() != "https" && target_url.scheme() == "https" {
        return false;
    }
    // CORP uses a schemeless site comparison, with the secure-response restriction
    // above. Reuse the core's PSL-aware comparison after normalizing the scheme.
    if target_url.set_scheme(source_url.scheme()).is_err() {
        return false;
    }
    // Site origins without a registrable domain (including IP addresses) require
    // the same origin; sharing an IP across ports does not authorize CORP.
    if !matches!(source_url.host(), Some(url::Host::Domain(host)) if psl::domain_str(host).is_some())
    {
        return same_origin(source, target_url.as_str());
    }
    same_site(source, target_url.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbo_surf_core::request_context::Destination;

    fn context(source: &str) -> RequestContext {
        RequestContext::resource(source, Destination::Script, RequestMode::NoCors)
    }

    #[test]
    fn corp_uses_native_origins_case_sensitive_single_header_and_psl_site() {
        let mut context = context("https://a.example.co.uk/page");
        let field = |value: &str| vec![("Cross-Origin-Resource-Policy".into(), value.into())];
        assert!(corp_allowed(
            &context,
            "https://a.example.co.uk/script",
            &field("same-origin")
        ));
        assert!(!corp_allowed(
            &context,
            "https://b.example.co.uk/script",
            &field("same-origin")
        ));
        assert!(corp_allowed(
            &context,
            "https://b.example.co.uk/script",
            &field("same-site")
        ));
        assert!(!corp_allowed(
            &context,
            "https://evil.co.uk/script",
            &field("same-site")
        ));
        assert!(corp_allowed(
            &context,
            "https://evil.co.uk/script",
            &field("Same-Origin")
        ));
        assert!(corp_allowed(
            &context,
            "https://evil.co.uk/script",
            &field("cross-origin")
        ));
        let duplicated = [field("same-origin"), field("same-site")].concat();
        assert!(corp_allowed(
            &context,
            "https://evil.co.uk/script",
            &duplicated
        ));
        context.opaque_origin = true;
        assert!(!corp_allowed(
            &context,
            "https://a.example.co.uk/script",
            &field("same-site")
        ));
        context.mode = RequestMode::Cors;
        assert!(corp_allowed(
            &context,
            "https://evil.co.uk/script",
            &field("same-origin")
        ));
    }

    #[test]
    fn same_site_rejects_insecure_initiators_of_secure_responses() {
        assert!(corp_same_site(
            "https://a.example.com",
            "http://b.example.com"
        ));
        assert!(!corp_same_site(
            "http://a.example.com",
            "https://b.example.com"
        ));
        assert!(!corp_same_site(
            "http://127.0.0.1:1234",
            "http://127.0.0.1:5678"
        ));
        assert!(!corp_same_site("http://127.0.0.1", "http://127.0.0.2"));
    }

    #[test]
    fn active_mixed_content_checks_secure_source_and_loopback_exemptions() {
        for target in [
            "http://localhost/file",
            "http://child.localhost/file",
            "http://127.12.3.4/file",
            "http://[::1]/file",
            "https://outside.test/file",
        ] {
            assert!(
                active_mixed_content_allowed("https://source.test", target),
                "{target}"
            );
        }
        for target in [
            "http://outside.test/file",
            "http://localhost.evil/file",
            "http://192.168.1.1/file",
            "http://0.0.0.0/file",
        ] {
            assert!(
                !active_mixed_content_allowed("https://source.test", target),
                "{target}"
            );
            assert!(
                active_mixed_content_allowed("http://source.test", target),
                "{target}"
            );
        }
    }
}

#[cfg(test)]
mod embedder_tests {
    use super::*;
    use turbo_surf_core::request_context::{Credentials, Destination};

    #[test]
    fn embedder_headers_parse_full_structured_items_and_reject_duplicates() {
        for value in [
            "require-corp",
            "require-corp; report-to=\"authored\"",
            "require-corp; optional",
            "require-corp; flag=?1",
            "require-corp; n=-12.34",
            "require-corp; bytes=:YQ==:",
        ] {
            assert_eq!(
                EmbedderPolicy::from_headers(&[(
                    "Cross-Origin-Embedder-Policy".into(),
                    value.into()
                )]),
                EmbedderPolicy::RequireCorp,
                "{value}"
            );
        }
        for value in [
            "\"require-corp\"",
            "Require-Corp",
            "require-corp, credentialless",
            "require-corp; flag=?9",
            "require-corp; report-to=\"unterminated",
            "require-corp; x=1000000000000000",
        ] {
            assert_eq!(
                EmbedderPolicy::from_headers(&[(
                    "Cross-Origin-Embedder-Policy".into(),
                    value.into()
                )]),
                EmbedderPolicy::UnsafeNone,
                "{value}"
            );
        }
        assert_eq!(
            EmbedderPolicy::from_headers(&[
                ("Cross-Origin-Embedder-Policy".into(), "require-corp".into()),
                ("Cross-Origin-Embedder-Policy".into(), "require-corp".into())
            ]),
            EmbedderPolicy::UnsafeNone
        );
    }

    #[test]
    fn credentialless_never_weakens_an_explicit_resource_policy() {
        let mut context = RequestContext::resource(
            "https://source.test",
            Destination::Script,
            RequestMode::NoCors,
        );
        context.credentialless = true;
        context.credentials = Credentials::Include;
        assert!(embedder_allowed(
            EmbedderPolicy::Credentialless,
            &context,
            "https://cross.test/script",
            &[]
        ));
        assert!(!embedder_allowed(
            EmbedderPolicy::Credentialless,
            &context,
            "https://cross.test/script",
            &[("Cross-Origin-Resource-Policy".into(), "same-origin".into())]
        ));
        assert!(!embedder_allowed(
            EmbedderPolicy::RequireCorp,
            &context,
            "https://cross.test/script",
            &[]
        ));
        context.mode = RequestMode::Cors;
        assert!(embedder_allowed(
            EmbedderPolicy::RequireCorp,
            &context,
            "https://cross.test/script",
            &[]
        ));
    }
}

#[cfg(test)]
mod measured_embedder_headers {
    use super::*;
    #[test]
    fn chrome155_structured_header_dialect_matches_native_policy_parsing() {
        let control: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../harness/detection/fixtures/coep-header-chrome155.json"
        ))
        .unwrap();
        for row in control["result"].as_array().unwrap() {
            let expected = if row["rows"][0]["allowed"] == false {
                EmbedderPolicy::RequireCorp
            } else if row["requests"][0]["cookie"] == "" {
                EmbedderPolicy::Credentialless
            } else {
                EmbedderPolicy::UnsafeNone
            };
            let value = row["policy"].as_str().unwrap();
            assert_eq!(
                EmbedderPolicy::from_headers(&[(
                    "Cross-Origin-Embedder-Policy".into(),
                    value.into()
                )]),
                expected,
                "{value}"
            );
        }
    }
}
