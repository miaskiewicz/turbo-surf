//! Deterministic fingerprint **seed pool**: a large set of internally-coherent
//! real-Chrome client identities, selected deterministically by a client key so
//! the SAME client always presents the SAME profile (stable), while the fleet
//! spreads across the pool (varied). This raises the *passive / consistency-only*
//! anti-bot bar — a server that cross-checks UA ↔ client hints ↔ `navigator.*`
//! sees one consistent browser, and at scale sees many distinct ones.
//!
//! It does **not** defeat active canvas/WebGL/audio draw-and-hash or PoW
//! challenges (those need a real raster backend / a token solver — out of scope;
//! see the tier-3 notes). The seed pool is the highest-value piece you can build
//! in-house with no Chromium.
//!
//! Coherence is the whole point: every [`Profile`] field agrees with the others
//! (UA OS ↔ `sec-ch-ua-platform` ↔ `navigator.platform`; Chrome major ↔
//! `sec-ch-ua`; `deviceMemory` within Chrome's privacy cap). A single `Profile`
//! drives BOTH the HTTP headers ([`crate::net`]) and the render-tier navigator —
//! **rotate both layers with the same profile**, or a page that fetches over HTTP
//! and then reads `navigator` sees a mismatch (itself a bot signal).

/// `Accept` header value — stable across Chrome versions and OSes, so it is not
/// part of the per-profile axes.
pub const ACCEPT: &str =
    "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,\
image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";

/// One coherent real-Chrome client identity. Construct via [`select`] (by client
/// key) or [`profile_at`] (by index); [`default_profile`] is the fixed identity
/// used when no profile is chosen (preserves pre-pool behaviour).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub chrome_major: u16,
    pub user_agent: String,
    pub sec_ch_ua: String,
    /// `sec-ch-ua-platform` value, already quoted (e.g. `"\"macOS\""`).
    pub sec_ch_ua_platform: &'static str,
    /// `navigator.platform` (e.g. `"MacIntel"`).
    pub nav_platform: &'static str,
    pub vendor: &'static str,
    pub accept_language: &'static str,
    pub languages: &'static [&'static str],
    pub hardware_concurrency: u8,
    /// `navigator.deviceMemory` — Chrome caps this at 8 for privacy, so the pool
    /// never emits a higher value (it would be a tell).
    pub device_memory: u8,
    pub max_touch_points: u8,
    pub screen_width: u16,
    pub screen_height: u16,
}

impl Profile {
    /// Chrome's top-level navigation headers for this identity, minus the ones the
    /// HTTP client owns (`accept-encoding`, `host`, `cookie`). The returned order
    /// is Chrome's; callers that store into a `BTreeMap` lose it (a known
    /// header-ordering limitation of the rustls path).
    pub fn nav_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("user-agent", self.user_agent.clone()),
            ("accept", ACCEPT.to_string()),
            ("accept-language", self.accept_language.to_string()),
            ("sec-ch-ua", self.sec_ch_ua.clone()),
            ("sec-ch-ua-mobile", "?0".to_string()),
            ("sec-ch-ua-platform", self.sec_ch_ua_platform.to_string()),
            ("sec-fetch-dest", "document".to_string()),
            ("sec-fetch-mode", "navigate".to_string()),
            ("sec-fetch-site", "none".to_string()),
            ("sec-fetch-user", "?1".to_string()),
            ("upgrade-insecure-requests", "1".to_string()),
        ]
    }
}

struct Os {
    /// UA platform token inside the `Mozilla/5.0 (...)` parens.
    ua_token: &'static str,
    /// Quoted `sec-ch-ua-platform` value.
    sec_ch_platform: &'static str,
    nav_platform: &'static str,
}

// Real desktop Chrome platform tokens. macOS pins `10_15_7` (Chrome freezes the
// reported macOS version there); Windows pins `10.0` (Win10/11 both report it).
const OSES: &[Os] = &[
    Os {
        ua_token: "Macintosh; Intel Mac OS X 10_15_7",
        sec_ch_platform: "\"macOS\"",
        nav_platform: "MacIntel",
    },
    Os {
        ua_token: "Windows NT 10.0; Win64; x64",
        sec_ch_platform: "\"Windows\"",
        nav_platform: "Win32",
    },
    Os {
        ua_token: "X11; Linux x86_64",
        sec_ch_platform: "\"Linux\"",
        nav_platform: "Linux x86_64",
    },
];

// Recent Chrome majors. Kept within wreq-util's emulated range so the same pool
// can later drive a per-profile TLS client without an unmatched version.
const MAJORS: &[u16] = &[153, 152, 151, 150, 149, 148];

// Plausible logical-core counts for desktops.
const CORES: &[u8] = &[4, 8, 12, 16];

// `navigator.deviceMemory` — Chrome only ever reports {4, 8} on desktop (capped).
const DEVICE_MEMORY: &[u8] = &[4, 8];

// Common real desktop resolutions (CSS px).
const SCREENS: &[(u16, u16)] = &[
    (1920, 1080),
    (2560, 1440),
    (1440, 900),
    (1536, 864),
    (1366, 768),
    (3840, 2160),
    (1680, 1050),
];

// (Accept-Language header, navigator.languages) pairs — kept consistent with each
// other.
const LANGS: &[(&str, &[&str])] = &[
    ("en-US,en;q=0.9", &["en-US", "en"]),
    ("en-GB,en;q=0.9", &["en-GB", "en"]),
    ("en-US,en;q=0.9,es;q=0.8", &["en-US", "en", "es"]),
    ("en-CA,en;q=0.9,fr-CA;q=0.8", &["en-CA", "en", "fr-CA"]),
];

/// Total number of distinct coherent profiles (the product of every axis).
pub fn pool_size() -> usize {
    OSES.len() * MAJORS.len() * CORES.len() * DEVICE_MEMORY.len() * SCREENS.len() * LANGS.len()
}

// FNV-1a (64-bit). A fixed, process-independent hash so a given key maps to the
// same profile across runs and machines — `std`'s default hasher is randomized
// and would break that guarantee.
fn fnv1a(key: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Pick the stable profile for a client key (host, session id, account, …). Same
/// key → same profile, forever; distinct keys spread across the pool.
pub fn select(key: &str) -> Profile {
    profile_at(fnv1a(key) as usize)
}

/// The profile at `index` (taken modulo [`pool_size`]). Mixed-radix decode over
/// the axes — every index yields a fully-coherent identity.
pub fn profile_at(index: usize) -> Profile {
    let mut i = index % pool_size();
    let os = &OSES[i % OSES.len()];
    i /= OSES.len();
    let major = MAJORS[i % MAJORS.len()];
    i /= MAJORS.len();
    let cores = CORES[i % CORES.len()];
    i /= CORES.len();
    let device_memory = DEVICE_MEMORY[i % DEVICE_MEMORY.len()];
    i /= DEVICE_MEMORY.len();
    let (screen_width, screen_height) = SCREENS[i % SCREENS.len()];
    i /= SCREENS.len();
    let (accept_language, languages) = LANGS[i % LANGS.len()];

    Profile {
        chrome_major: major,
        user_agent: format!(
            "Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
            os.ua_token, major
        ),
        sec_ch_ua: format!(
            "\"Google Chrome\";v=\"{m}\", \"Chromium\";v=\"{m}\", \"Not)A;Brand\";v=\"24\"",
            m = major
        ),
        sec_ch_ua_platform: os.sec_ch_platform,
        nav_platform: os.nav_platform,
        vendor: "Google Inc.",
        accept_language,
        languages,
        hardware_concurrency: cores,
        device_memory,
        max_touch_points: 0,
        screen_width,
        screen_height,
    }
}

/// The fixed identity used when no profile is selected: Chrome 153 on macOS,
/// matching the tier-1 default headers and the render-tier navigator. Keeping
/// this stable means turning the pool *off* (passing no key) reproduces the
/// pre-pool wire behaviour exactly.
pub fn default_profile() -> Profile {
    Profile {
        chrome_major: 153,
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
                     (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36"
            .to_string(),
        sec_ch_ua: "\"Google Chrome\";v=\"153\", \"Chromium\";v=\"153\", \"Not)A;Brand\";v=\"24\""
            .to_string(),
        sec_ch_ua_platform: "\"macOS\"",
        nav_platform: "MacIntel",
        vendor: "Google Inc.",
        accept_language: "en-US,en;q=0.9",
        languages: &["en-US", "en"],
        hardware_concurrency: 8,
        device_memory: 8,
        max_touch_points: 0,
        screen_width: 1920,
        screen_height: 1080,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_is_large() {
        // The user asked for ~2000 distinct seeds; the axes give well over that.
        assert!(pool_size() > 2000, "pool size {}", pool_size());
    }

    #[test]
    fn selection_is_deterministic_and_spread() {
        assert_eq!(select("client-A"), select("client-A"));
        // Distinct keys spread across the pool — compare the whole profile (the UA
        // string alone only encodes OS × major, so it understates the spread).
        let n = (0..500)
            .map(|i| format!("{:?}", select(&format!("client-{i}"))))
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        assert!(n > 100, "only {n} distinct profiles over 500 keys");
    }

    #[test]
    fn every_profile_is_internally_coherent() {
        // Sweep the whole pool: UA, client hints, and navigator must all agree.
        for idx in 0..pool_size() {
            let p = profile_at(idx);
            let major = p.chrome_major.to_string();
            assert!(p.user_agent.contains(&format!("Chrome/{major}.")));
            assert!(p.sec_ch_ua.contains(&format!("\"{major}\"")));
            // UA OS token ↔ sec-ch-ua-platform ↔ navigator.platform.
            let (ua_os, sec, nav) = if p.user_agent.contains("Macintosh") {
                ("Macintosh", "\"macOS\"", "MacIntel")
            } else if p.user_agent.contains("Windows") {
                ("Windows", "\"Windows\"", "Win32")
            } else {
                ("Linux", "\"Linux\"", "Linux x86_64")
            };
            assert!(p.user_agent.contains(ua_os));
            assert_eq!(p.sec_ch_ua_platform, sec);
            assert_eq!(p.nav_platform, nav);
            // deviceMemory must stay within Chrome's privacy cap.
            assert!(p.device_memory <= 8);
            assert_eq!(p.vendor, "Google Inc.");
        }
    }

    #[test]
    fn default_profile_matches_chrome_153_macos() {
        let p = default_profile();
        assert_eq!(p.chrome_major, 153);
        assert!(p.user_agent.contains("Chrome/153") && p.user_agent.contains("Macintosh"));
        assert_eq!(p.sec_ch_ua_platform, "\"macOS\"");
        // The default's headers are exactly the tier-1 set.
        let h = p.nav_headers();
        assert!(h.iter().any(|(k, _)| *k == "sec-fetch-mode"));
        assert!(!h.iter().any(|(k, _)| *k == "accept-encoding"));
    }
}
