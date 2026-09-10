//! Live e2e for the `trust-anchors` feature: hits a public TLS fingerprint echo
//! and asserts we emit (or omit) Chrome 152+'s `trust_anchors` ClientHello
//! extension (codepoint 0xCA34 / 51764) depending on the feature.
//!
//! - WITH `trust-anchors`: JA4 becomes `t13d1517h2` (one extra extension) and
//!   codepoint 51764 appears in the echoed extension list.
//! - WITHOUT it (impersonate only): JA4 is `t13d1516h2` and 51764 is absent.
//!
//! The extension count is the *only* JA4 delta — ciphers and ALPN are unchanged
//! — because wreq-util's Chrome149 profile predates the extension. JA4's
//! extension hash is order-independent, so it does not matter that BoringSSL
//! appends the extension at the tail of the ClientHello.
//!
//! Live network: auto-skips (does not fail) when offline, matching the repo's
//! harness convention. The whole file compiles away unless built with
//! `--features impersonate`.
#![cfg(feature = "impersonate")]

use turbo_surf_core::net::{fetch_html, FetchOptions};

// Echoes back the caller's observed TLS (JA3/JA4) fingerprint + extension list.
const ECHO: &str = "https://tls.peet.ws/api/all";

#[tokio::test]
async fn trust_anchors_extension_matches_feature() {
    let opts = FetchOptions {
        allow_non_html: true, // the echo serves application/json
        ..Default::default()
    };
    let body = match fetch_html(ECHO, opts).await {
        Ok(r) => r.html,
        Err(e) => {
            eprintln!("skipping trust-anchors e2e (network unavailable): {e}");
            return;
        }
    };
    let json: serde_json::Value = serde_json::from_str(&body).expect("echo returned non-JSON");
    let ja4 = json["tls"]["ja4"].as_str().unwrap_or_default();
    // JA4_a — the human-readable prefix before the cipher/extension hashes; it
    // carries the extension *count*, which is where trust_anchors shows up.
    let ja4_a = ja4.split('_').next().unwrap_or_default();

    // The echo lists the extension by its decimal codepoint (51764 == 0xCA34).
    let has_trust_anchors = body.contains("51764");

    if cfg!(feature = "trust-anchors") {
        assert_eq!(
            ja4_a, "t13d1517h2",
            "with the feature, JA4 should gain the trust_anchors extension (17): {ja4}"
        );
        assert!(
            has_trust_anchors,
            "trust_anchors (51764) should be present in the ClientHello: {body}"
        );
    } else {
        assert_eq!(
            ja4_a, "t13d1516h2",
            "without the feature, JA4 should match stock wreq-util Chrome149 (16): {ja4}"
        );
        assert!(
            !has_trust_anchors,
            "trust_anchors (51764) must not be sent without the feature: {body}"
        );
    }
}
