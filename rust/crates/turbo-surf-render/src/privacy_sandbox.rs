//! Private State Token issuer queries (privacy_sandbox.js). The browsing profile
//! records which issuers each top-frame origin has queried, capped like Chrome's
//! trust token store; no token is ever issued or redeemed by this engine.
use super::*;

/// Chrome's per-top-frame-origin issuer association limit.
const ISSUERS_PER_TOP_FRAME: usize = 2;

/// An issuer's origin when it is HTTP(S) and potentially trustworthy.
fn issuer_origin(issuer: &str) -> Option<String> {
    let url = url::Url::parse(issuer).ok()?;
    (matches!(url.scheme(), "http" | "https") && trustworthy_url(url.as_str()))
        .then(|| url.origin().ascii_serialization())
}

#[op2]
#[string]
pub(super) fn op_private_token_issuer(#[string] issuer: String) -> Option<String> {
    issuer_origin(&issuer)
}

/// The URL of the top-level document of the browsing context `context` belongs to.
fn top_frame_url<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &OpState,
    mut context: v8::Local<'s, v8::Context>,
) -> Option<String> {
    while let Some(parent) = context.get_slot::<FrameParent>() {
        context = parent.0.to_local(scope)?;
    }
    Some(
        context
            .get_slot::<FrameDocumentUrl>()
            .map(|url| url.0.clone())
            .unwrap_or_else(|| state.borrow::<Base>().0.clone()),
    )
}

/// 0 resolves (no tokens are held), 1 has no HTTP(S) top-frame origin, 2 exceeds the
/// top-frame origin's issuer associations. Redemption queries never associate.
#[op2(fast)]
pub(super) fn op_private_token_query(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
    #[string] issuer: String,
    associate: bool,
) -> u32 {
    let Some(context) = document.get_creation_context(scope) else {
        return 1;
    };
    let top = top_frame_url(scope, state, context)
        .and_then(|url| url::Url::parse(&url).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    let Some(top) = top else {
        return 1;
    };
    if !associate {
        return 0;
    }
    if !state.has::<BrowserStorage>() {
        state.put(BrowserStorage::default());
    }
    let storage = state.borrow::<BrowserStorage>().clone();
    let Ok(mut areas) = storage.0.lock() else {
        return 2;
    };
    let issuers = areas
        .private_token_issuers
        .entry(top.origin().ascii_serialization())
        .or_default();
    if issuers.contains(&issuer) {
        return 0;
    }
    if issuers.len() >= ISSUERS_PER_TOP_FRAME {
        return 2;
    }
    issuers.push(issuer);
    0
}

#[cfg(test)]
mod tests {
    use super::issuer_origin;

    #[test]
    fn issuers_are_http_family_potentially_trustworthy_origins() {
        for (issuer, origin) in [
            ("https://ONE.example:443/x?q", Some("https://one.example")),
            ("http://localhost:1/", Some("http://localhost:1")),
            ("http://a.localhost", Some("http://a.localhost")),
            ("http://[::1]", Some("http://[::1]")),
            ("http://127.0.0.1:5", Some("http://127.0.0.1:5")),
            ("http://issuer.example", None),
            ("wss://x.example", None),
            ("file:///x", None),
            ("relative", None),
        ] {
            assert_eq!(issuer_origin(issuer).as_deref(), origin, "{issuer}");
        }
    }
}
