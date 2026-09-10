//! Trusted-`__Secure-ENID` mint + cache for the native google `web_search` path.
//!
//! google's `/search` serves the real SERP to a plain NATIVE (no-browser) wreq
//! request *iff* it carries a **trusted** `__Secure-ENID` cookie; without one it
//! returns the `enablejs` JS shell. A trusted ENID is minted only by a real
//! browser's homepage load (the headed Chrome sidecar) but is long-lived (~2027)
//! and client-agnostic, so we mint it RARELY via the sidecar and reuse it across
//! many native searches. This module owns:
//!   * [`EnidCookie`] — the minted cookie record (persisted + injected into a jar),
//!   * [`EnidCache`] — a gitignored on-disk cache with `get_valid` / `set`,
//!   * [`mint_enid_cmd`] — the sidecar `{"mint":true}` contract (env-independent
//!     core, so it's unit-testable with a stub command).
//!
//! Chromium never enters the engine binary: minting shells out to the same
//! `TURBO_SURF_BROWSER_FETCH_CMD` sidecar the browser SERP path uses.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The cookie the sidecar earns from a real google homepage load and we replay on
/// the native `/search` fetch. `expires` is epoch SECONDS (`None` == a session
/// cookie), matching Playwright's `cookie.expires` and `CookieJar::add`'s units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnidCookie {
    pub name: String,
    pub value: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default = "default_path")]
    pub path: String,
    /// Epoch SECONDS; `None` (or a negative value from Playwright) == session.
    #[serde(default)]
    pub expires: Option<f64>,
}

fn default_domain() -> String {
    ".google.com".to_string()
}
fn default_path() -> String {
    "/".to_string()
}

/// The cookie name that ALONE gates the real SERP (AEC/SOCS are cached too but not
/// required — see the module doc). A cache without a live one of these is a miss.
pub const ENID_NAME: &str = "__Secure-ENID";

impl EnidCookie {
    /// True while the cookie is still usable at `now_secs` (session cookies never
    /// expire in-process). A negative `expires` (Playwright's session sentinel) is
    /// treated as no-expiry.
    fn is_live(&self, now_secs: f64) -> bool {
        match self.expires {
            Some(e) if e >= 0.0 => e > now_secs,
            _ => true,
        }
    }
}

/// Epoch seconds now (monotonic-enough for a ~2-year cookie horizon).
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// On-disk cache of the minted cookies. Persisted to a gitignored JSON file so a
/// trusted ENID survives engine restarts (mint is expensive + burns a real
/// browser session; reuse is the whole point).
#[derive(Clone, Debug)]
pub struct EnidCache {
    path: PathBuf,
}

/// The persisted shape: the cookie set + when it was minted (informational).
#[derive(Serialize, Deserialize, Default)]
struct CacheFile {
    #[serde(default)]
    cookies: Vec<EnidCookie>,
    #[serde(default)]
    minted_at: f64,
}

impl EnidCache {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The default cache location: `<dir>/.enid-cache.json` (gitignored), next to
    /// the sidecar. `TURBO_SURF_ENID_CACHE` overrides the full path; `dir` is the
    /// sidecar dir (default `scripts/browser-sidecar`), relative to the cwd.
    pub fn default_for(dir: &str) -> Self {
        match std::env::var("TURBO_SURF_ENID_CACHE") {
            Ok(p) if !p.is_empty() => Self::new(p),
            _ => Self::new(Path::new(dir).join(".enid-cache.json")),
        }
    }

    /// The cached cookies if a live `__Secure-ENID` is present, else `None` (a
    /// cache miss → the caller mints). Expired members are dropped from the set.
    pub fn get_valid(&self, now_secs: f64) -> Option<Vec<EnidCookie>> {
        let raw = std::fs::read_to_string(&self.path).ok()?;
        let file: CacheFile = serde_json::from_str(&raw).ok()?;
        let live: Vec<EnidCookie> = file
            .cookies
            .into_iter()
            .filter(|c| c.is_live(now_secs))
            .collect();
        let has_enid = live.iter().any(|c| c.name == ENID_NAME);
        if has_enid {
            Some(live)
        } else {
            None
        }
    }

    /// Persist a freshly minted cookie set (atomic-ish: write to a temp then rename).
    pub fn set(&self, cookies: &[EnidCookie]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = CacheFile {
            cookies: cookies.to_vec(),
            minted_at: now_secs(),
        };
        let body = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("serialize enid cache: {e}"))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &body).map_err(|e| format!("write enid cache: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("commit enid cache: {e}"))
    }
}

/// Mint a trusted `__Secure-ENID` via the sidecar `{"mint":true}` contract.
///
/// Writes `{"mint":true,"headless"?:bool}\n` to the sidecar's stdin; the sidecar
/// launches real Chrome (headed by default — a trusted ENID needs a real homepage
/// load), lands on `https://www.google.com/`, and writes
/// `{"cookies":[{name,value,domain?,path?,expires?}, …]}` to stdout. We require a
/// `__Secure-ENID` in the returned set (that's the whole reason to mint). This is
/// the env-independent core; the caller reads the command from
/// `TURBO_SURF_BROWSER_FETCH_CMD`.
pub async fn mint_enid_cmd(cmd: &str, headless: Option<bool>) -> Result<Vec<EnidCookie>, String> {
    use tokio::io::AsyncWriteExt;
    let mut parts = cmd.split_whitespace();
    let prog = parts
        .next()
        .ok_or("TURBO_SURF_BROWSER_FETCH_CMD is empty")?;
    let mut child = tokio::process::Command::new(prog)
        .args(parts)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn mint sidecar ({prog}): {e}"))?;
    let req = match headless {
        Some(h) => json!({ "mint": true, "headless": h }),
        None => json!({ "mint": true }),
    }
    .to_string();
    child
        .stdin
        .take()
        .ok_or("no sidecar stdin")?
        .write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "mint sidecar failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stdout)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    parse_mint_output(&out.stdout)
}

/// Parse a sidecar mint stdout into the cookie set, requiring a `__Secure-ENID`.
fn parse_mint_output(stdout: &[u8]) -> Result<Vec<EnidCookie>, String> {
    let v: Value = serde_json::from_slice(stdout)
        .map_err(|e| format!("mint sidecar returned non-JSON: {e}"))?;
    let arr = v
        .get("cookies")
        .and_then(Value::as_array)
        .ok_or("mint sidecar response has no 'cookies' array")?;
    let cookies: Vec<EnidCookie> = arr
        .iter()
        .filter_map(|c| serde_json::from_value(c.clone()).ok())
        .collect();
    if !cookies.iter().any(|c| c.name == ENID_NAME) {
        return Err(format!(
            "mint sidecar returned no {ENID_NAME} cookie (got {} cookie(s)); the homepage \
             load did not earn a trusted token — check the sidecar has a display (headed)",
            cookies.len()
        ));
    }
    Ok(cookies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    // A unique cache path under the OS temp dir (no tempdir dep). The parent is
    // created by `set`; each test uses a distinct file so they don't collide.
    fn tmp_cache_path() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let uniq = format!(
            "turbo-surf-enid-{}-{}-{}.json",
            std::process::id(),
            now_secs() as u64,
            N.fetch_add(1, Ordering::Relaxed)
        );
        std::env::temp_dir().join(uniq)
    }

    #[test]
    fn set_then_get_roundtrips_live_cookie() {
        let cache = EnidCache::new(tmp_cache_path());
        let future = now_secs() + 86_400.0;
        cache
            .set(&[EnidCookie {
                name: ENID_NAME.into(),
                value: "tok".into(),
                domain: ".google.com".into(),
                path: "/".into(),
                expires: Some(future),
            }])
            .unwrap();
        let got = cache.get_valid(now_secs()).expect("cache hit");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].value, "tok");
    }

    #[test]
    fn get_valid_is_none_for_expired_enid() {
        let cache = EnidCache::new(tmp_cache_path());
        let past = now_secs() - 10.0;
        cache
            .set(&[EnidCookie {
                name: ENID_NAME.into(),
                value: "stale".into(),
                domain: ".google.com".into(),
                path: "/".into(),
                expires: Some(past),
            }])
            .unwrap();
        assert!(cache.get_valid(now_secs()).is_none(), "expired → miss");
    }

    #[test]
    fn get_valid_is_none_without_enid() {
        let cache = EnidCache::new(tmp_cache_path());
        cache
            .set(&[EnidCookie {
                name: "AEC".into(),
                value: "x".into(),
                domain: ".google.com".into(),
                path: "/".into(),
                expires: None,
            }])
            .unwrap();
        // AEC alone is not sufficient — no trusted ENID means a miss.
        assert!(cache.get_valid(now_secs()).is_none());
    }

    #[test]
    fn session_cookie_never_expires() {
        let cache = EnidCache::new(tmp_cache_path());
        cache
            .set(&[EnidCookie {
                name: ENID_NAME.into(),
                value: "sess".into(),
                domain: ".google.com".into(),
                path: "/".into(),
                expires: None,
            }])
            .unwrap();
        assert!(cache.get_valid(now_secs() + 1e12).is_some());
    }

    #[test]
    fn missing_cache_file_is_miss_not_error() {
        let cache = EnidCache::new("/nonexistent/dir/.enid-cache.json");
        assert!(cache.get_valid(now_secs()).is_none());
    }

    #[test]
    fn parse_mint_output_requires_enid() {
        let ok = parse_mint_output(
            br#"{"cookies":[{"name":"__Secure-ENID","value":"t"},{"name":"AEC","value":"a"}]}"#,
        )
        .unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[0].name, ENID_NAME);
        // Defaults fill domain/path when the sidecar omits them.
        assert_eq!(ok[0].domain, ".google.com");
        assert_eq!(ok[0].path, "/");

        let err = parse_mint_output(br#"{"cookies":[{"name":"AEC","value":"a"}]}"#).unwrap_err();
        assert!(err.contains("no __Secure-ENID"), "{err}");

        assert!(parse_mint_output(b"not json").is_err());
        assert!(parse_mint_output(b"{}").unwrap_err().contains("cookies"));
    }

    #[tokio::test]
    async fn mint_enid_cmd_drives_stub_sidecar() {
        // Stub sidecar: drains stdin, prints a cookie set with a trusted ENID.
        // Whitespace-free JS token so the command survives the split.
        let cmd = "node -e process.stdin.resume();process.stdout.write(JSON.stringify({cookies:[{name:'__Secure-ENID',value:'minted'}]}))";
        let cookies = mint_enid_cmd(cmd, None).await.unwrap();
        assert_eq!(cookies[0].value, "minted");
    }

    #[tokio::test]
    async fn mint_enid_cmd_forwards_headless() {
        let cmd = "node -e d='';process.stdin.on('data',c=>d+=c);process.stdin.on('end',()=>{h=JSON.parse(d).headless;process.stdout.write(JSON.stringify({cookies:[{name:'__Secure-ENID',value:'h='+h}]}))})";
        let on = mint_enid_cmd(cmd, Some(true)).await.unwrap();
        assert_eq!(on[0].value, "h=true");
        let off = mint_enid_cmd(cmd, None).await.unwrap();
        assert_eq!(off[0].value, "h=undefined");
    }
}
