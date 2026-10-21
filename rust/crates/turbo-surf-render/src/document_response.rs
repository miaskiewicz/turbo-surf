//! Authentic navigation metadata supplied by the host that fetched the document.

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct DocumentResponseMetadata {
    pub status: u16,
    /// The referrer already selected for the actual navigation request.
    pub referrer: String,
}
impl Default for DocumentResponseMetadata {
    fn default() -> Self {
        Self {
            status: 200,
            referrer: String::new(),
        }
    }
}

pub(crate) fn report_url(value: &str) -> String {
    let Ok(mut url) = url::Url::parse(value) else {
        return value.to_string();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return url.scheme().to_string();
    }
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.to_string()
}

pub(crate) fn local_navigation_referrer(
    source: &str,
    policy: turbo_surf_core::referrer_policy::ReferrerPolicy,
) -> String {
    use turbo_surf_core::referrer_policy::ReferrerPolicy;
    let Ok(url) = url::Url::parse(source) else {
        return String::new();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return String::new();
    }
    match policy {
        ReferrerPolicy::NoReferrer | ReferrerPolicy::SameOrigin => String::new(),
        ReferrerPolicy::UnsafeUrl | ReferrerPolicy::NoReferrerWhenDowngrade => {
            ReferrerPolicy::UnsafeUrl
                .referrer(source, source)
                .unwrap_or_default()
        }
        _ => format!("{}/", url.origin().ascii_serialization()),
    }
}

impl DocumentResponseMetadata {
    /// The native request state after redirects determines the document's referrer.
    pub fn from_request(
        status: u16,
        request: &turbo_surf_core::request_context::RequestContext,
        target: &str,
    ) -> Self {
        Self {
            status,
            referrer: request
                .source
                .as_deref()
                .and_then(|source| {
                    request
                        .referrer_policy
                        .referrer(request.referrer.as_deref().unwrap_or(source), target)
                })
                .unwrap_or_default(),
        }
    }
}
