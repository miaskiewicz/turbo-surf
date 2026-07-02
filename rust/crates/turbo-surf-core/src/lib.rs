//! turbo-surf-core — Rust port, **tier 1**: the browserless native-speed crawler
//! core. Ports the pure-logic + networking modules of the JS library:
//!
//! - [`url`] — resolve / canonicalize / http-gate (frontier dedupe basis)
//! - [`frontier`] — canonical-dedup URL queue with depth
//! - [`robots`] — robots.txt parse + longest-match + TTL cache
//! - [`cookies`] — RFC 6265 subset CookieJar
//! - [`net`] — `fetch_html` over reqwest (charset/byte-cap/type-gate/cookies)
//! - [`crawl`] — frontier-driven scheduling (concurrency, politeness, backoff)
//!
//! The JS-execution tier (deno_core) and the napi/playwright shim are later
//! tiers; the page fetch+parse seam is the [`crawl::Navigator`] trait, which the
//! tier-2 `Page` (on the turbo-dom Rust crate) will implement.

pub mod akamai;
pub mod aws_waf;
pub mod cache;
pub mod challenge;
pub mod cloudflare;
pub mod cookies;
pub mod crawl;
pub mod fingerprint;
pub mod frontier;
pub mod measure;
pub mod net;
pub mod robots;
pub mod url;

/// The active HTTP backend, re-exported so this crate's `net` module and every
/// downstream crate type against one alias instead of an extern crate — the
/// backend then swaps in a single place. Default is stock `reqwest` (rustls);
/// with the `impersonate` feature it becomes `wreq` (BoringSSL), which forges a
/// real Chrome TLS/JA3/JA4 + HTTP-2 fingerprint. Their client/response/builder
/// surfaces line up closely enough that `net` compiles against either. (Named
/// `http_backend`, not `http`, to avoid colliding with the extern `http` crate.)
#[cfg(not(feature = "impersonate"))]
pub use reqwest as http_backend;
#[cfg(feature = "impersonate")]
pub use wreq as http_backend;

/// Library version — kept in lockstep with `package.json` per the release rules.
pub const VERSION: &str = "0.3.3";
