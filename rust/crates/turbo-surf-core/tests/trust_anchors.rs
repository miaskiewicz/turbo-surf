//! Live echo smoke check for Chrome 154's built-in trust-anchor extension.
//! The deterministic ClientHello regression lives in client_hello.rs. The legacy
//! `trust-anchors` feature is now an alias: actual root IDs are always sent on
//! the impersonation backend, matching its Chromium certificate store.
#![cfg(feature = "impersonate")]

use turbo_surf_core::net::{fetch_html, FetchOptions};

// Echoes back the caller's observed TLS (JA3/JA4) fingerprint + extension list.
const ECHO: &str = "https://tls.peet.ws/api/all";

#[tokio::test]
async fn chrome154_trust_anchors_extension_is_present() {
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

    assert_eq!(
        ja4_a, "t13d1517h2",
        "unexpected Chrome 154 JA4 prefix: {ja4}"
    );
    assert!(has_trust_anchors, "trust_anchors (51764) missing: {body}");
}
