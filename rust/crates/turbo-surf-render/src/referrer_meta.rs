use turbo_surf_core::referrer_policy::ReferrerPolicy;

/// Meta keywords also include the HTML legacy aliases; padding is not trimmed.
pub(crate) fn policy(value: &str) -> Option<ReferrerPolicy> {
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "never" => Some(ReferrerPolicy::NoReferrer),
        "always" => Some(ReferrerPolicy::UnsafeUrl),
        "default" => Some(ReferrerPolicy::default()),
        "origin-when-crossorigin" => Some(ReferrerPolicy::OriginWhenCrossOrigin),
        _ => ReferrerPolicy::parse(&value),
    }
}
