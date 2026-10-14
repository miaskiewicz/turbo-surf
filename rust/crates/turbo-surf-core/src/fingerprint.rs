//! Deterministic fingerprint **seed pool**: a large set of internally-coherent
//! real-Chrome client identities, selected deterministically by a client key so
//! the SAME client always presents the SAME profile (stable), while the fleet
//! spreads across the pool (varied). This raises the *passive / consistency-only*
//! anti-bot bar — a server that cross-checks UA ↔ client hints ↔ `navigator.*`
//! sees one consistent browser, and at scale sees many distinct ones.
//!
//! It does **not** defeat active canvas/WebGL/audio draw-and-hash or PoW
//! challenges by itself. The optional raster/GPU bridges and render-tier solvers
//! handle separate parts of those workloads; acceptance must still be measured.
//!
//! Coherence is the whole point: every [`Profile`] field agrees with the others
//! (UA OS ↔ `sec-ch-ua-platform` ↔ `navigator.platform`; Chrome major ↔
//! `sec-ch-ua`; `deviceMemory` within Chrome's privacy cap). A single `Profile`
//! should drive both HTTP and JS identity. Per-call rotation currently applies
//! only to rustls headers: wreq emulation pins default_profile(). Pin that same
//! identity in the render tier on the wreq path to avoid a cross-layer mismatch.

/// Navigation Accept for the current configured Chrome release.
pub const ACCEPT: &str = crate::browser::RELEASES[0].navigation_accept;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BrandVersion {
    pub brand: String,
    pub version: String,
}

/// Chrome's major-seeded GREASE brand/version permutation, shared by HTTP and JS.
/// Source: Chromium 155 components/embedder_support/user_agent_utils.cc.
pub fn brand_versions(major: u16, version: &str, full: bool) -> Vec<BrandVersion> {
    const CHARACTERS: [&str; 11] = [" ", "(", ":", "-", ".", "/", ")", ";", "=", "?", "_"];
    const VERSIONS: [&str; 3] = ["8", "99", "24"];
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let seed = usize::from(major);
    let grease = VERSIONS[seed % VERSIONS.len()];
    let brands = [
        BrandVersion {
            brand: format!(
                "Not{}A{}Brand",
                CHARACTERS[seed % 11],
                CHARACTERS[(seed + 1) % 11]
            ),
            version: if full {
                format!("{grease}.0.0.0")
            } else {
                grease.into()
            },
        },
        BrandVersion {
            brand: "Chromium".into(),
            version: version.into(),
        },
        BrandVersion {
            brand: "Google Chrome".into(),
            version: version.into(),
        },
    ];
    let mut output = vec![brands[0].clone(); 3];
    for (brand, destination) in brands.into_iter().zip(ORDERS[seed % 6]) {
        output[destination] = brand;
    }
    output
}

pub fn brand_header(major: u16, version: &str, full: bool) -> String {
    brand_versions(major, version, full)
        .into_iter()
        .map(|item| format!("\"{}\";v=\"{}\"", item.brand, item.version))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One coherent real-Chrome client identity. Construct via [`select`] (by client
/// key) or [`profile_at`] (by index); [`default_profile`] is the fixed identity
/// used when no profile is chosen (preserves pre-pool behaviour).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub chrome_major: u16,
    /// Full build version for high-entropy UA-CH (`fullVersionList` /
    /// `uaFullVersion`), from the per-version [`crate::browser`] registry, e.g.
    /// `"155.0.8059.79"`. The UA string stays reduced to `<major>.0.0.0`.
    pub full_version: &'static str,
    /// Top-level navigation header ORDER for this Chrome version (a fingerprint),
    /// from the [`crate::browser`] registry — Chrome reorders it across releases.
    pub header_order: &'static [&'static str],
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
    /// `navigator.deviceMemory` in GiB. Chrome 155 desktop rounds within 2..32;
    /// the local default and the selected pool machine can have different memory.
    pub device_memory: u8,
    pub max_touch_points: u8,
    pub screen_width: u16,
    pub screen_height: u16,
}

impl Profile {
    /// UA-CH architecture for the selected desktop platform.
    pub fn architecture(&self) -> &'static str {
        if self.nav_platform == "MacIntel" {
            if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
                "x86"
            } else {
                "arm"
            }
        } else {
            "x86"
        }
    }

    /// A native macOS session uses its actual product version. Foreign-platform
    /// profiles retain their configured defaults; the frozen UA OS token is separate.
    pub fn platform_version(&self) -> String {
        match self.nav_platform {
            "MacIntel" => macos_product_version().to_string(),
            "Win32" => "10.0.0".to_string(),
            _ => String::new(),
        }
    }

    /// Chrome's top-level navigation headers for this identity, minus the ones the
    /// HTTP client owns (`accept-encoding`, `host`, `cookie`). The returned order
    /// is Chrome's; callers that store into a `BTreeMap` lose it (a known
    /// header-ordering limitation of the rustls path).
    pub fn nav_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("user-agent", self.user_agent.clone()),
            (
                "accept",
                crate::browser::release(self.chrome_major)
                    .navigation_accept
                    .to_string(),
            ),
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

fn macos_product_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        #[cfg(target_os = "macos")]
        if let Ok(plist) =
            std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist")
        {
            if let Some(rest) = plist.split("<key>ProductVersion</key>").nth(1) {
                if let Some(value) = rest
                    .split("<string>")
                    .nth(1)
                    .and_then(|s| s.split("</string>").next())
                {
                    if let Some(version) = normalize_product_version(value) {
                        return version;
                    }
                }
            }
        }
        "15.0.0".to_string()
    })
}

#[cfg(any(target_os = "macos", test))]
fn normalize_product_version(value: &str) -> Option<String> {
    let mut parts: Vec<&str> = value.trim().split('.').collect();
    if parts.len() > 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    parts.resize(3, "0");
    Some(parts.join("."))
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

// Configured release majors only: older unlisted versions cannot borrow a newer
// full version while still advertising their original major. Transport emulation
// remains pinned to the current default identity.
const MAJORS: &[u16] = &[155, 154, 153];

// Plausible logical-core counts for desktops.
const CORES: &[u8] = &[4, 8, 12, 16];

// Selected pool machines have 4/8 GiB; this is a subset of Chrome's allowed values.
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

    let rel = crate::browser::release(major);
    Profile {
        chrome_major: major,
        full_version: rel.full_version,
        header_order: rel.header_order,
        user_agent: format!(
            "Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
            os.ua_token, major
        ),
        sec_ch_ua: brand_header(major, &major.to_string(), false),
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

/// The fixed identity used when no profile is selected: the current stable Chrome
/// ([`crate::browser::DEFAULT_MAJOR`]) on macOS, matching the tier-1 default headers
/// and the render-tier navigator. Its version-specific data (full build, header
/// order) comes from the [`crate::browser`] registry, so bumping `DEFAULT_MAJOR`
/// flips the wire + render default coherently.
pub fn default_profile() -> Profile {
    let rel = crate::browser::default_release();
    let major = rel.major;
    Profile {
        chrome_major: major,
        full_version: rel.full_version,
        header_order: rel.header_order,
        user_agent: format!(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
        ),
        sec_ch_ua: brand_header(major, &major.to_string(), false),
        sec_ch_ua_platform: "\"macOS\"",
        nav_platform: "MacIntel",
        vendor: "Google Inc.",
        accept_language: "en-US,en;q=0.9",
        languages: &["en-US", "en"],
        hardware_concurrency: 8,
        device_memory: rel.default_device_memory,
        max_touch_points: 0,
        screen_width: 1920,
        screen_height: 1080,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome155_brand_headers_match_the_captured_low_and_full_versions() {
        assert_eq!(
            brand_header(155, "155", false),
            "\"Google Chrome\";v=\"155\", \"Chromium\";v=\"155\", \"Not(A:Brand\";v=\"24\""
        );
        assert_eq!(brand_header(155, "155.0.8059.79", true), "\"Google Chrome\";v=\"155.0.8059.79\", \"Chromium\";v=\"155.0.8059.79\", \"Not(A:Brand\";v=\"24.0.0.0\"");
        assert_eq!(
            brand_header(154, "154", false),
            "\"Chromium\";v=\"154\", \"Google Chrome\";v=\"154\", \"Not A(Brand\";v=\"99\""
        );
    }

    #[test]
    fn product_versions_are_normalized_without_accepting_markup() {
        assert_eq!(normalize_product_version("27.2"), Some("27.2.0".into()));
        assert_eq!(normalize_product_version(" 15.0.1 "), Some("15.0.1".into()));
        for value in ["", "27..2", "27.2.extra", "27.2.0.1", "<string>"] {
            assert_eq!(normalize_product_version(value), None);
        }
    }

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
            // These selected pool machines have 4/8 GiB of memory.
            assert!(p.device_memory <= 8);
            assert_eq!(p.vendor, "Google Inc.");
        }
    }

    #[test]
    fn default_profile_matches_current_stable_macos() {
        let p = default_profile();
        let major = crate::browser::DEFAULT_MAJOR;
        assert_eq!(p.chrome_major, major);
        assert!(
            p.user_agent.contains(&format!("Chrome/{major}")) && p.user_agent.contains("Macintosh")
        );
        assert!(p.sec_ch_ua.contains(&format!("\"{major}\"")));
        assert_eq!(p.sec_ch_ua_platform, "\"macOS\"");
        assert_eq!(
            p.device_memory, 16,
            "Measured local Chrome 155 desktop memory"
        );
        // Version-specific data comes from the browser registry.
        assert_eq!(
            p.full_version,
            crate::browser::default_release().full_version
        );
        assert_eq!(
            p.header_order,
            crate::browser::default_release().header_order
        );
        // The default's headers are exactly the tier-1 set.
        let h = p.nav_headers();
        assert!(h.iter().any(|(k, _)| *k == "sec-fetch-mode"));
        assert!(!h.iter().any(|(k, _)| *k == "accept-encoding"));
    }

    #[test]
    fn profile_header_order_tracks_its_major() {
        // A 154 profile carries the 154 order; a 153 profile the 153 order — coherent
        // per-version sets, driven by the registry.
        for idx in 0..pool_size() {
            let p = profile_at(idx);
            assert_eq!(
                p.header_order,
                crate::browser::release(p.chrome_major).header_order
            );
            assert_eq!(
                p.full_version,
                crate::browser::release(p.chrome_major).full_version
            );
        }
    }
}
