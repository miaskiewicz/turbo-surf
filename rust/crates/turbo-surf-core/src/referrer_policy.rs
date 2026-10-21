//! Referrer selection shared by native requests and followed redirects.
use url::Url;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReferrerPolicy {
    NoReferrer,
    NoReferrerWhenDowngrade,
    SameOrigin,
    Origin,
    StrictOrigin,
    OriginWhenCrossOrigin,
    #[default]
    StrictOriginWhenCrossOrigin,
    UnsafeUrl,
}
impl ReferrerPolicy {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "no-referrer" => Self::NoReferrer,
            "no-referrer-when-downgrade" => Self::NoReferrerWhenDowngrade,
            "same-origin" => Self::SameOrigin,
            "origin" => Self::Origin,
            "strict-origin" => Self::StrictOrigin,
            "origin-when-cross-origin" => Self::OriginWhenCrossOrigin,
            "strict-origin-when-cross-origin" => Self::StrictOriginWhenCrossOrigin,
            "unsafe-url" => Self::UnsafeUrl,
            _ => return None,
        })
    }
    /// The last recognized response header token wins; invalid fields do not reset policy.
    pub fn from_header(value: &str) -> Option<Self> {
        value
            .split(',')
            .rev()
            .find_map(|value| Self::parse(value.trim()))
    }
    pub fn referrer(self, source: &str, target: &str) -> Option<String> {
        let mut source = Url::parse(source).ok()?;
        let target = Url::parse(target).ok()?;
        if !matches!(source.scheme(), "http" | "https")
            || !matches!(target.scheme(), "http" | "https")
        {
            return None;
        }
        source.set_fragment(None);
        let _ = source.set_username("");
        let _ = source.set_password(None);
        let origin = format!("{}/", source.origin().ascii_serialization());
        let full = if source.as_str().len() > 4096 {
            origin.clone()
        } else {
            source.to_string()
        };
        let same_origin = source.origin() == target.origin();
        let downgrade = crate::request_context::trustworthy(source.as_str())
            && !crate::request_context::trustworthy(target.as_str());
        match self {
            Self::NoReferrer => None,
            Self::SameOrigin => same_origin.then_some(full),
            Self::NoReferrerWhenDowngrade => (!downgrade).then_some(full),
            Self::Origin => Some(origin),
            Self::StrictOrigin => (!downgrade).then_some(origin),
            Self::OriginWhenCrossOrigin => Some(if same_origin { full } else { origin }),
            Self::StrictOriginWhenCrossOrigin if same_origin => Some(full),
            Self::StrictOriginWhenCrossOrigin => (!downgrade).then_some(origin),
            Self::UnsafeUrl => Some(full),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_selection_preserves_url_privacy_and_origin_without_mutating_credentials() {
        let source = "https://user:secret@source.test/path?q=authored#fragment";
        let same = "https://source.test/next";
        let cross = "https://other.test/next";
        let insecure = "http://other.test/next";
        let full = "https://source.test/path?q=authored";
        let origin = "https://source.test/";
        for (policy, expected) in [
            (ReferrerPolicy::NoReferrer, [None, None, None]),
            (ReferrerPolicy::SameOrigin, [Some(full), None, None]),
            (
                ReferrerPolicy::Origin,
                [Some(origin), Some(origin), Some(origin)],
            ),
            (
                ReferrerPolicy::StrictOrigin,
                [Some(origin), Some(origin), None],
            ),
            (
                ReferrerPolicy::NoReferrerWhenDowngrade,
                [Some(full), Some(full), None],
            ),
            (
                ReferrerPolicy::OriginWhenCrossOrigin,
                [Some(full), Some(origin), Some(origin)],
            ),
            (
                ReferrerPolicy::StrictOriginWhenCrossOrigin,
                [Some(full), Some(origin), None],
            ),
            (
                ReferrerPolicy::UnsafeUrl,
                [Some(full), Some(full), Some(full)],
            ),
        ] {
            for (target, expected) in [same, cross, insecure].into_iter().zip(expected) {
                assert_eq!(
                    policy.referrer(source, target).as_deref(),
                    expected,
                    "{policy:?}: {target}"
                );
            }
        }
    }
    #[test]
    fn trusted_loopback_and_long_urls_follow_the_standard_referrer_reduction() {
        assert_eq!(
            ReferrerPolicy::StrictOrigin
                .referrer("https://source.test/page", "http://127.0.0.1:8000/"),
            Some("https://source.test/".into())
        );
        assert_eq!(
            ReferrerPolicy::UnsafeUrl.referrer(
                &format!("https://source.test/{}", "x".repeat(4096)),
                "https://source.test/next"
            ),
            Some("https://source.test/".into())
        );
        assert_eq!(
            ReferrerPolicy::UnsafeUrl.referrer("data:text/html,source", "https://target.test/"),
            None
        );
    }
    #[test]
    fn response_header_uses_last_valid_exact_policy_without_unknown_token_fallback() {
        assert_eq!(
            ReferrerPolicy::from_header("origin, no-referrer, invalid"),
            Some(ReferrerPolicy::NoReferrer)
        );
        assert_eq!(ReferrerPolicy::from_header("ORIGIN, empty"), None);
        assert_eq!(ReferrerPolicy::parse(""), None);
    }
}
