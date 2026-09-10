//! Network-layer fingerprint e2e for the `impersonate` feature: hits a public
//! TLS/HTTP-2 fingerprint echo and asserts wreq presents a *Chrome* JA4 + Akamai
//! HTTP-2 fingerprint — the WAF-facing behaviour the Tier-1 headers alone can't
//! provide (rustls can't forge the ClientHello). This is the check that a WAF
//! doing Akamai-style fingerprinting would run against us.
//!
//! Live network: auto-skips (does not fail) when offline, matching the repo's
//! harness convention. The whole file compiles away unless built with
//! `--features impersonate`.
#![cfg(feature = "impersonate")]

use turbo_surf_core::net::{fetch_html, FetchOptions};

// Echoes back the caller's observed TLS (JA3/JA4) + HTTP/2 (Akamai) fingerprint.
const ECHO: &str = "https://tls.peet.ws/api/all";

#[tokio::test]
async fn presents_a_chrome_tls_and_http2_fingerprint() {
    let opts = FetchOptions {
        allow_non_html: true, // the echo serves application/json
        ..Default::default()
    };
    let body = match fetch_html(ECHO, opts).await {
        Ok(r) => r.html,
        Err(e) => {
            eprintln!("skipping fingerprint e2e (network unavailable): {e}");
            return;
        }
    };
    let json: serde_json::Value = serde_json::from_str(&body).expect("echo returned non-JSON");

    // JA4: a TLS 1.3 ClientHello shaped like Chrome's (`t13d…`). The stock rustls
    // client yields a different JA4, so this only passes through wreq emulation.
    let ja4 = json["tls"]["ja4"].as_str().unwrap_or_default();
    assert!(ja4.starts_with("t13d"), "unexpected JA4: {ja4}");

    // Akamai HTTP/2 fingerprint: the pseudo-header order `m,a,s,p` is Chrome's
    // and is stable across Chrome versions — a strong, low-brittleness browser
    // tell that a generic HTTP/2 client (e.g. plain h2) does not reproduce.
    let akamai = json["http2"]["akamai_fingerprint"]
        .as_str()
        .unwrap_or_default();
    assert!(
        akamai.ends_with("|m,a,s,p"),
        "HTTP/2 fingerprint not Chrome-shaped: {akamai}"
    );

    // ...and the UA wreq advertises is Chrome, consistent with the TLS layer (a
    // UA/JA4 mismatch is itself a classic bot tell).
    let ua = json["user_agent"].as_str().unwrap_or_default();
    assert!(ua.contains("Chrome/"), "unexpected UA: {ua}");

    // `emulate` pins the wire UA to `fingerprint::default_profile` so the reported
    // version stays current even though wreq's bundled emulation lags a few Chrome
    // versions behind. Regression guard: the on-wire UA must be exactly that pinned
    // identity (not wreq's older bundled UA), keeping every layer on one version.
    let expected_ua = turbo_surf_core::fingerprint::default_profile().user_agent;
    assert_eq!(
        ua, expected_ua,
        "wire UA is wreq's bundled UA, not the pinned default_profile override"
    );
}
