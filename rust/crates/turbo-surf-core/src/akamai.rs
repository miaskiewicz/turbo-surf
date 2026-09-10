//! In-house **Akamai Bot Manager** solver — the first hand-written
//! [`crate::challenge::ChallengeSolver`] (vs renting Hyper/Scrapfly or the browser
//! sidecar). Akamai is the best feasibility:value target: ubiquitous on
//! retail/travel/airline/banking, the most publicly reverse-engineered of the
//! hard vendors, and largely *static* reads + a known `sensor_data` format.
//!
//! The hard part is [`generate_sensor`]: build the `sensor_data` payload Akamai's
//! `_abck` cookie machinery expects. The solver then POSTs it to the page's
//! sensor endpoint and reads the cleared `_abck` from the response.
//!
//! TDD: the tests below are the contract. They are RED until `generate_sensor`
//! and `AkamaiSolver::solve` are implemented. Use the render-tier `probe` mode on
//! a live `_abck` script to confirm which fields are static before filling them.

use crate::challenge::{Challenge, ChallengeSolver, SolveContext, SolveError, SolvedToken};
use crate::http_backend as http;
use std::time::Duration;

/// A stored Akamai `sensor_data` encoding generation. Akamai's format has shifted
/// across major generations; each is a distinct on-the-wire shape, so we keep a
/// **versioned registry** rather than one format — [`SensorVersion::latest`] is the
/// default and the harness exercises [`SensorVersion::all`].
///
/// - **V1** (web BMP 1.6x–1.7x): plaintext, `;`-delimited sections + integrity hash.
/// - **V2** (2.1.x–2.2.x): a `:`-joined field array, **PRNG-shuffled** with a seed
///   derived from the script hash, then reassembled (the real 2.x obfuscation).
/// - **V3** (3.x): an **encrypted** blob (keystream keyed by the script hash),
///   base64-wrapped — the shape of the AES-encrypted v3 payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorVersion {
    V1,
    V2,
    V3,
}

impl SensorVersion {
    /// The version string Akamai stamps for this generation.
    pub fn label(self) -> &'static str {
        match self {
            SensorVersion::V1 => "1.74",
            SensorVersion::V2 => "2.2.3",
            SensorVersion::V3 => "3.0.0",
        }
    }

    /// Newest stored generation (the solver default).
    pub fn latest() -> Self {
        SensorVersion::V3
    }

    /// Every stored generation, oldest → newest (the harness sweeps these).
    pub fn all() -> &'static [SensorVersion] {
        &[SensorVersion::V1, SensorVersion::V2, SensorVersion::V3]
    }
}

/// Inputs to the `sensor_data` generator: the identity + the seed values Akamai
/// hands out on the first (challenge) response.
#[derive(Debug, Clone, Default)]
pub struct SensorInput {
    pub user_agent: String,
    pub page_url: String,
    /// The `_abck` cookie value from the challenge response (the seed to refresh).
    pub abck: String,
    /// The `bm_sz` cookie value from the challenge response.
    pub bm_sz: String,
    /// Hash extracted from the page's Akamai script — Akamai seeds its 2.x PRNG
    /// shuffle and 3.x key from it. Empty → derived from the UA (deterministic).
    pub script_hash: String,
}

/// Build the `sensor_data` payload using [`SensorVersion::latest`].
pub fn generate_sensor(input: &SensorInput) -> String {
    generate_sensor_versioned(input, SensorVersion::latest())
}

/// Build the `sensor_data` payload for a specific [`SensorVersion`]. Deterministic
/// for a fixed input (replayable per client).
///
/// NOTE: each version reproduces the real *structure* of that Akamai generation
/// (delimiters, shuffle, encryption shape) + the full POST/parse flow; the exact
/// field set + key schedule a live edge validates must still be keyed off the
/// page's script (use the render-tier `probe`).
pub fn generate_sensor_versioned(input: &SensorInput, version: SensorVersion) -> String {
    let fields = base_fields(input, version);
    let seed = if input.script_hash.is_empty() {
        fnv1a(&input.user_agent)
    } else {
        fnv1a(&input.script_hash)
    };
    match version {
        SensorVersion::V1 => encode_v1(&fields),
        SensorVersion::V2 => encode_v2(&fields, seed),
        SensorVersion::V3 => encode_v3(&fields, seed),
    }
}

// The common field set every generation carries (identity + seeds + telemetry),
// each tagged so a decoder can locate it. The version label leads.
fn base_fields(input: &SensorInput, version: SensorVersion) -> Vec<String> {
    vec![
        version.label().to_string(),
        format!("uar,{}", input.user_agent),
        format!("abck,{}", input.abck),
        format!("bmsz,{}", input.bm_sz),
        "scr,1920,1080,24".to_string(),
        "mm,0,0,1,1,2,3,5,8".to_string(), // synthesized pointer path
        format!("href,{}", input.page_url),
    ]
}

// V1: plaintext `;`-delimited sections, closed with an integrity hash.
fn encode_v1(fields: &[String]) -> String {
    let payload = fields.join(";");
    format!("{payload};{:016x}", fnv1a(&payload))
}

// V2: `:`-join the fields, then PRNG-shuffle them with a script-hash seed (Akamai
// 2.x shuffles the element array with a file-hash-seeded PRNG), and stamp the seed
// so the edge can reproduce the permutation.
fn encode_v2(fields: &[String], seed: u64) -> String {
    let mut joined: Vec<String> = fields.iter().map(|f| f.replace(':', "_")).collect();
    shuffle(&mut joined, seed);
    format!("{}:{:016x}", joined.join(":"), seed)
}

// V3: build the v2 string, then run it through a keystream keyed by the script hash
// and base64-wrap it — the shape of the encrypted v3 blob (`{seed}.{ciphertext}`).
fn encode_v3(fields: &[String], seed: u64) -> String {
    let plain = encode_v2(fields, seed);
    let cipher = keystream_xor(plain.as_bytes(), seed);
    format!("3.{}.{}", seed, base64(&cipher))
}

// FNV-1a (64-bit) — deterministic, process-independent.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// A small LCG, seeded deterministically — stands in for Akamai's file-hash-seeded
// PRNG used to shuffle the field array.
fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

// Fisher–Yates shuffle driven by the seeded LCG (deterministic for a fixed seed).
fn shuffle<T>(v: &mut [T], seed: u64) {
    let mut st = seed ^ 0x9e37_79b9_7f4a_7c15;
    for i in (1..v.len()).rev() {
        let j = (lcg(&mut st) % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
}

// Keystream XOR keyed by the seed — a deterministic, reversible transform standing
// in for the v3 AES blob (structure, not Akamai's exact key schedule).
fn keystream_xor(data: &[u8], seed: u64) -> Vec<u8> {
    let mut st = seed;
    data.iter().map(|b| b ^ (lcg(&mut st) as u8)).collect()
}

// Minimal standard base64 (no padding dependency).
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Solves Akamai walls by generating `sensor_data` and POSTing it to the sensor
/// endpoint (defaults to the page URL), then reading the cleared `_abck`.
pub struct AkamaiSolver {
    sensor_url: Option<String>,
    version: SensorVersion,
    client: http::Client,
}

impl AkamaiSolver {
    pub fn new() -> Self {
        Self {
            sensor_url: None,
            version: SensorVersion::latest(),
            client: crate::net::build_client(),
        }
    }
    /// Override the sensor POST endpoint (defaults to the challenge page URL).
    pub fn with_sensor_url(mut self, url: String) -> Self {
        self.sensor_url = Some(url);
        self
    }
    /// Pin the `sensor_data` generation (defaults to [`SensorVersion::latest`]).
    pub fn with_version(mut self, version: SensorVersion) -> Self {
        self.version = version;
        self
    }
}

impl Default for AkamaiSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for AkamaiSolver {
    fn name(&self) -> &'static str {
        "akamai"
    }

    async fn solve(&self, ch: &Challenge, ctx: &SolveContext) -> Result<SolvedToken, SolveError> {
        let sensor = generate_sensor_versioned(
            &SensorInput {
                user_agent: ctx.user_agent.clone(),
                page_url: ch.page_url.clone(),
                abck: String::new(),
                bm_sz: String::new(),
                script_hash: String::new(),
            },
            self.version,
        );
        // Akamai posts sensor_data back to the page (or a configured sensor path);
        // the edge replies with a refreshed _abck.
        let url = self
            .sensor_url
            .clone()
            .unwrap_or_else(|| ch.page_url.clone());
        let body = serde_json::json!({ "sensor_data": sensor }).to_string();
        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;

        let cookies: Vec<(String, String)> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|line| {
                line.strip_prefix("_abck=").map(|rest| {
                    let val = rest.split(';').next().unwrap_or("").to_string();
                    ("_abck".to_string(), val)
                })
            })
            .collect();
        if cookies.is_empty() {
            return Err(SolveError::Parse("no _abck in sensor response".into()));
        }
        Ok(SolvedToken {
            cookies,
            headers: Vec::new(),
            ttl: Duration::from_secs(30 * 60),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> SensorInput {
        SensorInput {
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36"
                .into(),
            page_url: "https://shop.example.com/".into(),
            abck: "0~seed~-1~-1".into(),
            bm_sz: "ABCDEF1234567890".into(),
            script_hash: "deadbeefcafe".into(),
        }
    }

    // V1 (plaintext) structural contract.
    #[test]
    fn sensor_v1_has_plaintext_structure() {
        let s = generate_sensor_versioned(&input(), SensorVersion::V1);
        assert!(s.starts_with("1.74"), "v1 must open with the version: {s}");
        assert!(s.contains(&input().user_agent), "v1 must embed the UA");
        assert!(s.contains("seed"), "v1 must carry the _abck seed");
        assert!(
            s.split(';').count() >= 8,
            "v1 must have the device/behavioral sections + hash"
        );
        let last = s.rsplit(';').next().unwrap_or("");
        assert!(
            !last.is_empty() && last.chars().all(|c| c.is_ascii_hexdigit()),
            "v1 must close with a hex integrity hash, got {last:?}"
        );
    }

    // The HARNESS: every stored version must produce a non-empty, deterministic
    // payload stamped with its own version generation, and the latest is V3
    // (encrypted shape). This is the "store multiple versions + harness passes all"
    // contract — fill a version's real encoding and this keeps the others green.
    #[test]
    fn every_version_generates_and_is_deterministic() {
        for &v in SensorVersion::all() {
            let a = generate_sensor_versioned(&input(), v);
            let b = generate_sensor_versioned(&input(), v);
            assert_eq!(a, b, "{:?} must be deterministic", v);
            assert!(!a.is_empty(), "{:?} produced empty payload", v);
            match v {
                // V1/V2 carry the version label inline; V3 is an encrypted blob
                // prefixed with its generation tag.
                SensorVersion::V1 => assert!(a.starts_with("1.74")),
                SensorVersion::V2 => {
                    assert!(a.starts_with("2.2.3"), "v2 label: {a}");
                    assert!(a.contains(':'), "v2 must be colon-joined");
                }
                SensorVersion::V3 => {
                    assert!(a.starts_with("3."), "v3 must be tagged 3.x: {a}");
                    // Opaque (encrypted) — the raw UA must NOT survive in clear.
                    assert!(!a.contains("Mozilla/5.0"), "v3 must be encrypted");
                }
            }
        }
        // Distinct generations must differ on the wire.
        let v1 = generate_sensor_versioned(&input(), SensorVersion::V1);
        let v2 = generate_sensor_versioned(&input(), SensorVersion::V2);
        let v3 = generate_sensor_versioned(&input(), SensorVersion::V3);
        assert!(v1 != v2 && v2 != v3 && v1 != v3, "versions must differ");
        // The default tracks the latest generation.
        assert_eq!(generate_sensor(&input()), v3);
    }

    // RED until solve() is implemented: generate -> POST sensor_data -> parse the
    // cleared _abck from the response Set-Cookie. Mock Akamai sensor endpoint.
    #[tokio::test]
    async fn solver_posts_sensor_and_parses_abck() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            // The POST body must carry the generated sensor_data.
            assert!(req.contains("sensor_data"), "expected a sensor POST: {req}");
            let body = "{}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: _abck=CLEARED~0~ok; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.flush().await;
        });

        let solver =
            AkamaiSolver::new().with_sensor_url(format!("http://127.0.0.1:{port}/akam/sensor"));
        let ch = Challenge {
            vendor: crate::challenge::Vendor::Akamai,
            page_url: format!("http://127.0.0.1:{port}/"),
        };
        let ctx = SolveContext {
            user_agent: input().user_agent,
            proxy: None,
        };
        let token = solver.solve(&ch, &ctx).await.unwrap();
        let abck = token.cookies.iter().find(|(k, _)| k == "_abck");
        assert!(abck.is_some(), "solve must return the cleared _abck");
        assert!(
            abck.unwrap().1.starts_with("CLEARED"),
            "must parse the issued _abck value"
        );
    }
}
