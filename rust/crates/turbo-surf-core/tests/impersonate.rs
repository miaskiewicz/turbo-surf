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

    // Request-#1 network parity vs real Chrome (the layer sorted BEFORE any JS runs).
    // The echo reflects the exact HTTP/2 HEADERS frame we sent, so we assert on it
    // directly: the sent header names (order preserved) and the HEADERS priority.
    let headers_frame = json["http2"]["sent_frames"]
        .as_array()
        .and_then(|frames| frames.iter().find(|f| f["frame_type"] == "HEADERS"))
        .expect("no HEADERS frame in echo");
    // Non-pseudo header names, in the order they went on the wire. Pseudo-headers
    // (":method" etc.) start with ':', so we drop them and take each name as the
    // text before the first ':' in "name: value".
    let names: Vec<&str> = headers_frame["headers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h.as_str())
        .filter(|h| !h.starts_with(':'))
        .map(|h| h.split(':').next().unwrap_or_default().trim())
        .collect();

    // 1. HEADERS-frame priority weight is Chrome's 256 (wire byte 255), not
    //    wreq-util's stock 220 — closed via the vendored wreq-util patch.
    assert_eq!(
        headers_frame["priority"]["weight"].as_u64(),
        Some(256),
        "HEADERS priority weight is not Chrome's 256"
    );

    // 2. `upgrade-insecure-requests: 1` is present, in Chrome's slot (right after the
    //    low-entropy sec-ch-ua trio, right before user-agent).
    let uir = names
        .iter()
        .position(|n| *n == "upgrade-insecure-requests")
        .expect("upgrade-insecure-requests missing on the wire");
    assert_eq!(
        names.get(uir - 1),
        Some(&"sec-ch-ua-platform"),
        "upgrade-insecure-requests not after sec-ch-ua-platform"
    );
    assert_eq!(
        names.get(uir + 1),
        Some(&"user-agent"),
        "upgrade-insecure-requests not before user-agent"
    );

    // 3. NO high-entropy client hints on a cold request. Real Chrome sends only the
    //    low-entropy trio + upgrade-insecure-requests until a server replies with
    //    `Accept-CH`; emitting the arch/bitness/model/full-version(-list) family
    //    unconditionally is itself a bot tell.
    for hint in [
        "sec-ch-ua-full-version-list",
        "sec-ch-ua-full-version",
        "sec-ch-ua-arch",
        "sec-ch-ua-platform-version",
        "sec-ch-ua-bitness",
        "sec-ch-ua-wow64",
        "sec-ch-ua-model",
        "sec-ch-ua-form-factors",
    ] {
        assert!(
            !names.contains(&hint),
            "high-entropy client hint leaked on a cold request: {hint}"
        );
    }

    // 4. The full Chrome-153 navigation header order, verbatim.
    let expected_order = [
        "sec-ch-ua",
        "sec-ch-ua-mobile",
        "sec-ch-ua-platform",
        "upgrade-insecure-requests",
        "user-agent",
        "accept",
        "sec-fetch-site",
        "sec-fetch-mode",
        "sec-fetch-user",
        "sec-fetch-dest",
        "accept-encoding",
        "accept-language",
        "priority",
    ];
    assert_eq!(
        names, expected_order,
        "header order diverges from Chrome 153"
    );

    // 5. Accept-Encoding uses Chrome's spelling/order, not tower-http's default.
    assert!(
        body.contains("gzip, deflate, br, zstd"),
        "accept-encoding is not Chrome's `gzip, deflate, br, zstd`"
    );
}
