//! Versioned Chrome release registry — the single source of truth for every
//! fingerprint value that DIFFERS between Chrome versions.
//!
//! The data lives in `browser_releases.json` (compiled by build.rs) and is
//! produced from measured Chrome identity batteries by
//! `harness/detection/reversion-chrome.mjs`; see
//! `harness/detection/CHROME-REVERSION.md`. [`crate::fingerprint`] and
//! [`crate::net`] use this registry; the render tier receives its defaults
//! through private host ops.
//!
//! What's version-specific (lives here): the reported **major**, the full build
//! version for high-entropy UA-CH (`fullVersionList` / `uaFullVersion`), the
//! top-level **navigation header order**, measured per-request-class orders, the
//! `Accept`/`Priority` values per destination, the local device-memory control and
//! the upstream transport recipe. What's NOT (stays in `fingerprint`/`net`): the
//! UA string *template* and `sec-ch-ua` *shape* (both derive purely from the
//! major), OS tokens, and the TLS/HTTP-2 configuration (owned by the pinned
//! transport recipe, independently measured against the target Chrome version).

/// One coherent Chrome release's version-specific fingerprint data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeRelease {
    /// Reported major (UA `Chrome/<major>.0.0.0`, `sec-ch-ua` `v="<major>"`).
    pub major: u16,
    /// Full build version for high-entropy UA-CH (`getHighEntropyValues`
    /// `fullVersionList` / `uaFullVersion`), e.g. `"154.0.8037.98"`. The UA STRING
    /// itself stays reduced to `<major>.0.0.0` (Chrome's UA-Reduction); this full
    /// version only surfaces through the client-hints high-entropy API.
    pub full_version: &'static str,
    /// Top-level navigation header order for this release (a fingerprint). wreq
    /// emits these first, in this order; a header absent from a request is skipped.
    pub header_order: &'static [&'static str],
    pub navigation_accept: &'static str,
    /// Device memory observed for the local desktop control, not a universal cap.
    pub default_device_memory: u8,
    /// Upstream wreq-util Chrome TLS/H2 recipe this release's measured transport
    /// reuses (the transport fork is only re-cut when the measured hello/frames change).
    pub transport_base_major: u16,
    /// Measured wire header order per request class (see [`request_class`]), with
    /// Chrome's HTTP/1.1 spelling. Empty for releases measured before the identity
    /// battery; those fall back to `header_order` for every request.
    pub request_orders: &'static [(&'static str, &'static [&'static str])],
    /// Measured `Accept` per `Sec-Fetch-Dest` (subresources; navigations use
    /// `navigation_accept`). Empty → the long-standing defaults in request_context.
    pub destination_accept: &'static [(&'static str, &'static str)],
    /// Measured `Priority` header per `Sec-Fetch-Dest`. Empty → request_context defaults.
    pub destination_priority: &'static [(&'static str, &'static str)],
}

impl ChromeRelease {
    /// Measured order for a request class, if this release recorded one.
    pub fn request_order(&self, class: &str) -> Option<&'static [&'static str]> {
        lookup(self.request_orders, class)
    }

    /// Measured `Accept` for a fetch destination, if recorded.
    pub fn accept_for(&self, destination: &str) -> Option<&'static str> {
        lookup(self.destination_accept, destination)
    }

    /// Measured `Priority` header for a fetch destination, if recorded.
    pub fn priority_for(&self, destination: &str) -> Option<&'static str> {
        lookup(self.destination_priority, destination)
    }
}

fn lookup<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table
        .iter()
        .find_map(|(name, value)| (*name == key).then_some(*value))
}

// DEFAULT_MAJOR and RELEASES, newest first, compiled from browser_releases.json.
include!(concat!(env!("OUT_DIR"), "/browser_releases.rs"));

/// High-entropy hints whose presence moves Chrome to its `-hinted` header order.
pub const HIGH_ENTROPY_HINTS: &[&str] = &[
    "sec-ch-ua-arch",
    "sec-ch-ua-bitness",
    "sec-ch-ua-full-version",
    "sec-ch-ua-full-version-list",
    "sec-ch-ua-model",
    "sec-ch-ua-platform-version",
    "sec-ch-ua-wow64",
    "sec-ch-ua-form-factors",
];

/// The request class whose measured order applies to a request: `navigate`,
/// `no-cors`, `cors-resource` (CORS script/style/font), `cors` (fetch/XHR) or
/// `preflight`, suffixed `-hinted` when high-entropy client hints are present.
/// Mirrors `requestClass` in harness/detection/identity-diff.mjs.
pub fn request_class(method: &str, header: impl Fn(&str) -> Option<String>) -> String {
    let mode = header("sec-fetch-mode").unwrap_or_default();
    let destination = header("sec-fetch-dest").unwrap_or_default();
    let base = if method.eq_ignore_ascii_case("OPTIONS")
        && header("access-control-request-method").is_some()
    {
        "preflight"
    } else {
        match mode.as_str() {
            // Non-trustworthy origins get no Fetch Metadata; navigations still upgrade.
            "" if header("upgrade-insecure-requests").is_some() => "navigate",
            "navigate" => "navigate",
            "no-cors" => "no-cors",
            _ if matches!(destination.as_str(), "script" | "style" | "font") => "cors-resource",
            _ => "cors",
        }
    };
    if HIGH_ENTROPY_HINTS.iter().any(|hint| header(hint).is_some()) {
        format!("{base}-hinted")
    } else {
        base.to_string()
    }
}

/// The [`ChromeRelease`] for `major`. Exact match if listed; otherwise the nearest
/// configured release **at or below** `major` (Chrome's header order + UA shape are
/// stable only within the configured release ranges, so an unlisted newer version
/// resolves to the latest configured profile without claiming a new transport audit). A `major` **older** than everything
/// listed resolves to the OLDEST listed release, whose "classic"-era config applies
/// to those earlier Chromes too. Never panics (`RELEASES` is non-empty).
pub fn release(major: u16) -> &'static ChromeRelease {
    if let Some(exact) = RELEASES.iter().find(|r| r.major == major) {
        return exact;
    }
    if let Some(below) = RELEASES
        .iter()
        .filter(|r| r.major <= major)
        .max_by_key(|r| r.major)
    {
        return below;
    }
    // Older than everything listed → the oldest configured release (classic era).
    RELEASES
        .iter()
        .min_by_key(|r| r.major)
        .unwrap_or(&RELEASES[0])
}

/// The default release (the current stable, [`DEFAULT_MAJOR`]).
pub fn default_release() -> &'static ChromeRelease {
    RELEASES
        .iter()
        .find(|r| r.major == DEFAULT_MAJOR)
        .unwrap_or(&RELEASES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_release_is_listed_and_newest_first() {
        assert_eq!(default_release().major, DEFAULT_MAJOR);
        assert!(default_release()
            .full_version
            .starts_with(&format!("{DEFAULT_MAJOR}.")));
        assert!(RELEASES
            .windows(2)
            .all(|pair| pair[0].major > pair[1].major));
    }

    #[test]
    fn release_exact_and_fallback() {
        let newest = RELEASES[0].major;
        let oldest = RELEASES.last().unwrap().major;
        for listed in RELEASES {
            assert_eq!(release(listed.major).major, listed.major);
        }
        // Older than everything listed → the OLDEST listed (classic-era) config.
        assert_eq!(release(oldest - 1).major, oldest);
        assert_eq!(release(100).major, oldest, "predates all → oldest listed");
        // Future/unlisted-newer → the newest listed config.
        assert_eq!(release(newest + 44).major, newest);
    }

    #[test]
    fn header_orders_differ_between_154_and_153() {
        // 154 reordered vs 153: accept-language sits before accept only in 154.
        let o154 = release(154).header_order;
        let o153 = release(153).header_order;
        assert_ne!(o154, o153, "154 header order must differ from 153");
        let position = |order: &[&str], name: &str| order.iter().position(|h| *h == name);
        assert!(position(o154, "accept-language") < position(o154, "accept"));
        assert!(position(o153, "accept") < position(o153, "accept-language"));
    }

    #[test]
    fn full_versions_match_their_major_and_cookie_keeps_its_slot() {
        for r in RELEASES {
            assert!(r.full_version.starts_with(&format!("{}.", r.major)));
            assert!(
                r.header_order.contains(&"cookie"),
                "{} cookie slot",
                r.major
            );
            for (_, order) in r.request_orders {
                assert!(!order.iter().any(|name| name.starts_with(':')));
            }
        }
    }

    #[test]
    fn chrome155_navigation_fields_preserve_the_measured_wire_order_and_image_accept() {
        let current = release(155);
        let position = |name: &str| current.header_order.iter().position(|h| *h == name);
        assert!(position("accept-encoding") < position("accept-language"));
        assert!(current.navigation_accept.contains("image/jxl"));
        assert!(!release(154).navigation_accept.contains("image/jxl"));
    }

    #[test]
    fn request_classes_follow_fetch_metadata_and_hints() {
        let class = |method: &str, pairs: &[(&str, &str)]| {
            request_class(method, |name| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| value.to_string())
            })
        };
        let mode = |value| ("sec-fetch-mode", value);
        let dest = |value| ("sec-fetch-dest", value);
        assert_eq!(
            class("GET", &[mode("navigate"), dest("document")]),
            "navigate"
        );
        assert_eq!(class("GET", &[mode("no-cors"), dest("image")]), "no-cors");
        assert_eq!(class("GET", &[mode("cors"), dest("font")]), "cors-resource");
        assert_eq!(class("POST", &[mode("cors"), dest("empty")]), "cors");
        assert_eq!(
            class(
                "OPTIONS",
                &[mode("cors"), ("access-control-request-method", "PUT")]
            ),
            "preflight"
        );
        assert_eq!(
            class("GET", &[mode("cors"), ("sec-ch-ua-arch", "\"arm\"")]),
            "cors-hinted"
        );
    }
}
