//! Versioned Chrome release registry — the single source of truth for every
//! fingerprint value that DIFFERS between Chrome versions.
//!
//! New releases require measured identity/header configuration and separate native
//! engine and transport validation. [`crate::fingerprint`] and [`crate::net`] use
//! this registry; the render tier receives its defaults through private host ops.
//!
//! What's version-specific (lives here): the reported **major**, the full build
//! version for high-entropy UA-CH (`fullVersionList` / `uaFullVersion`), and the
//! top-level **navigation header order** (Chrome reorders it across releases — a
//! real, JA-style fingerprint). What's NOT (stays in `fingerprint`/`net`): the UA
//! string *template* and `sec-ch-ua` *shape* (both derive purely from the major),
//! OS tokens, and the TLS/HTTP-2 configuration (owned by the pinned transport recipe,
//! independently measured against the target Chrome version).

/// The reported version when no explicit major is chosen — the current stable
/// Chrome. Bump this when a newer release is added to [`RELEASES`].
pub const DEFAULT_MAJOR: u16 = 155;

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
}

const ACCEPT_154: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";
const ACCEPT_155: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,image/jxl,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";

// Chrome 154 (historical measured profile): `accept-language` is hoisted up right after
// `user-agent` (before `accept`); `accept-encoding` sits late (before cookie /
// priority). Verified against a live Chrome 154 on-wire h2 capture.
const HEADER_ORDER_154: &[&str] = &[
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "sec-ch-ua-platform",
    "upgrade-insecure-requests",
    "user-agent",
    "accept-language",
    "accept",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-user",
    "sec-fetch-dest",
    "accept-encoding",
    "cookie",
    "priority",
];

// Chrome 153 and earlier (the "classic" order Chrome held for many releases):
// `accept` immediately after `user-agent`; `accept-language` deferred to near the
// end. Kept correct so switching to a 153 identity stays coherent.
const HEADER_ORDER_153: &[&str] = &[
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
    "cookie",
    "priority",
];

/// Every configured Chrome release, newest first. Each entry is fully coherent on
/// its own. Add a new version by dropping a literal here.
pub const RELEASES: &[ChromeRelease] = &[
    ChromeRelease {
        major: 155,
        full_version: "155.0.8059.79",
        // Real localhost H2 capture on 2026-10-13 restores the classic order.
        header_order: HEADER_ORDER_153,
        navigation_accept: ACCEPT_155,
        default_device_memory: 16,
    },
    ChromeRelease {
        major: 154,
        full_version: "154.0.8037.98", // installed Chrome oracle, captured 2026-10-09
        header_order: HEADER_ORDER_154,
        navigation_accept: ACCEPT_154,
        default_device_memory: 8,
    },
    ChromeRelease {
        major: 153,
        full_version: "153.0.7938.132", // Chrome 153 build (realistic-shaped; 155 is the default)
        header_order: HEADER_ORDER_153,
        navigation_accept: ACCEPT_154,
        default_device_memory: 8,
    },
];

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
    fn default_release_is_current_major() {
        assert_eq!(default_release().major, DEFAULT_MAJOR);
        assert_eq!(default_release().major, 155);
        assert!(default_release().full_version.starts_with("155."));
    }

    #[test]
    fn release_exact_and_fallback() {
        assert_eq!(release(154).major, 154);
        assert_eq!(release(155).major, 155);
        assert_eq!(release(153).major, 153);
        // Older than everything listed → the OLDEST listed (classic-era) config.
        assert_eq!(
            release(152).major,
            153,
            "152 -> oldest listed (153-era classic)"
        );
        assert_eq!(release(100).major, 153, "predates all → oldest listed");
        // Future/unlisted-newer → the newest listed config.
        assert_eq!(release(200).major, 155, "future/unlisted -> newest listed");
    }

    #[test]
    fn header_orders_differ_between_154_and_153() {
        // The whole point: 154 reordered vs 153. accept-language sits earlier in 154.
        let o154 = release(154).header_order;
        let o153 = release(153).header_order;
        assert_ne!(o154, o153, "154 header order must differ from 153");
        let al154 = o154.iter().position(|&h| h == "accept-language").unwrap();
        let a154 = o154.iter().position(|&h| h == "accept").unwrap();
        assert!(al154 < a154, "154: accept-language before accept");
        let al153 = o153.iter().position(|&h| h == "accept-language").unwrap();
        let a153 = o153.iter().position(|&h| h == "accept").unwrap();
        assert!(a153 < al153, "153: accept before accept-language");
    }

    #[test]
    fn full_versions_match_their_major() {
        for r in RELEASES {
            assert!(
                r.full_version.starts_with(&format!("{}.", r.major)),
                "{} full_version {} must start with the major",
                r.major,
                r.full_version
            );
        }
    }

    #[test]
    fn chrome155_navigation_fields_preserve_the_measured_wire_order_and_image_accept() {
        let current = release(155);
        let encoding = current
            .header_order
            .iter()
            .position(|name| *name == "accept-encoding")
            .unwrap();
        let language = current
            .header_order
            .iter()
            .position(|name| *name == "accept-language")
            .unwrap();
        assert!(encoding < language);
        assert!(current.navigation_accept.contains("image/jxl"));
        assert!(!release(154).navigation_accept.contains("image/jxl"));
    }
}
