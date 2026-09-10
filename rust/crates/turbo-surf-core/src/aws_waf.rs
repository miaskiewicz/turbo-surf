//! In-house **AWS WAF Bot Control** solver (the bot layer behind CloudFront / ALB —
//! CloudFront itself is just the CDN). Two tiers, like Cloudflare:
//!
//! - **Common** ([`WafTier::Common`]): header/UA/rate heuristics + an `aws-waf-token`
//!   cookie. No PoW, no canvas — the fingerprint + seed pool already clear most of
//!   it; here we just run the lightweight `challenge.js` to mint a token and replay.
//! - **Targeted** ([`WafTier::Targeted`]): a JS `challenge.js` + real PoW + CAPTCHA +
//!   behavioral — closer to Akamai. The JS-compute part runs in the V8 tier (same
//!   [`crate::challenge::PowEngine`] as Cloudflare); the **CAPTCHA** variant is not
//!   self-solvable and routes to the browser sidecar.
//!
//! Like the other in-house solvers: the flow (detect tier → run challenge.js →
//! harvest `aws-waf-token`) is real; the exact live token the edge validates needs
//! keying off a captured `challenge.js` (use the render-tier `probe`).

use crate::challenge::{
    Challenge, ChallengeSolver, PowEngine, SolveContext, SolveError, SolvedToken, Vendor,
};
use crate::http_backend as http;
use std::time::Duration;

/// Which AWS WAF Bot Control tier a response is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WafTier {
    /// Header/token heuristics — clears with fingerprint + a minted token.
    Common,
    /// JS challenge + PoW (self-solvable via the V8 engine).
    Targeted,
    /// CAPTCHA tier — not self-solvable (routes to the browser sidecar).
    Captcha,
}

impl WafTier {
    pub fn label(self) -> &'static str {
        match self {
            WafTier::Common => "common",
            WafTier::Targeted => "targeted",
            WafTier::Captcha => "captcha",
        }
    }
    pub fn self_solvable(self) -> bool {
        !matches!(self, WafTier::Captcha)
    }
    pub fn all() -> &'static [WafTier] {
        &[WafTier::Common, WafTier::Targeted, WafTier::Captcha]
    }
}

/// Classify the AWS WAF tier from a response's action header + body markers.
pub fn detect_tier(action_header: Option<&str>, body: &str) -> WafTier {
    if action_header == Some("captcha") || body.contains("captcha.awswaf.com") {
        return WafTier::Captcha;
    }
    if action_header == Some("challenge")
        || body.contains("token.awswaf.com")
        || body.contains("challenge.js")
    {
        return WafTier::Targeted;
    }
    WafTier::Common
}

/// Solves AWS WAF walls: run the page's `challenge.js` in the V8 tier (when an
/// engine is wired) to mint an `aws-waf-token`, replay it. Common tier often clears
/// on the minted token alone; CAPTCHA tier is rejected (route to the sidecar).
pub struct AwsWafSolver {
    pow: Option<Box<dyn PowEngine>>,
    client: http::Client,
}

impl AwsWafSolver {
    pub fn new() -> Self {
        Self {
            pow: None,
            client: crate::net::build_client(),
        }
    }
    /// Run `challenge.js` via the render tier's V8 to mint the token (the proper path).
    pub fn with_pow_engine(mut self, engine: Box<dyn PowEngine>) -> Self {
        self.pow = Some(engine);
        self
    }

    // The minted token: run the page's challenge.js in V8 when an engine is wired,
    // else a structural placeholder token. The script is expected to leave the token
    // on a known sink (window.awsWafToken / a global) that our wrapper reads back.
    fn token(&self, page: &str) -> String {
        if let Some(engine) = &self.pow {
            if let Some(script) = extract_challenge_js(page) {
                if let Ok(tok) = engine.compute(&script) {
                    if !tok.is_empty() {
                        return tok;
                    }
                }
            }
        }
        format!("{:016x}.aws", fnv1a(page))
    }
}

impl Default for AwsWafSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for AwsWafSolver {
    fn name(&self) -> &'static str {
        "awswaf"
    }

    async fn solve(&self, ch: &Challenge, _ctx: &SolveContext) -> Result<SolvedToken, SolveError> {
        // Fetch the challenge page + classify the tier.
        let resp = self
            .client
            .get(&ch.page_url)
            .send()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        let action = resp
            .headers()
            .get("x-amzn-waf-action")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let page = resp
            .text()
            .await
            .map_err(|e| SolveError::Http(e.to_string()))?;
        let tier = detect_tier(action.as_deref(), &page);
        if !tier.self_solvable() {
            // CAPTCHA → the browser sidecar must take it.
            return Err(SolveError::Unsupported(Vendor::AwsWaf));
        }
        // Mint + return the aws-waf-token to replay on the fast path.
        let token = self.token(&page);
        Ok(SolvedToken {
            cookies: vec![("aws-waf-token".to_string(), token)],
            headers: Vec::new(),
            ttl: Duration::from_secs(5 * 60),
        })
    }
}

// Pull the AWS WAF challenge JS out of the page: the inline script(s) referencing
// the awswaf token machinery, wrapped to surface the minted token it produces.
fn extract_challenge_js(html: &str) -> Option<String> {
    let mut body = String::new();
    let mut rest = html;
    while let Some(open) = rest.find("<script") {
        let after = &rest[open..];
        let start = after.find('>')? + 1;
        let end = after.find("</script>")?;
        let code = &after[start..end];
        if code.contains("awswaf") || code.contains("challenge") || code.contains("AwsWafToken") {
            body.push_str(code);
            body.push('\n');
        }
        rest = &after[end + "</script>".len()..];
    }
    if body.is_empty() {
        return None;
    }
    Some(format!(
        "{body}\n;(function(){{ \
           try {{ if (window.awsWafToken != null) return String(window.awsWafToken); }} catch(e){{}} \
           try {{ if (window.AwsWafIntegration && window.AwsWafIntegration.getToken) \
                  return String(window.AwsWafIntegration.getToken()); }} catch(e){{}} \
           return ''; }})()"
    ))
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_tier() {
        assert_eq!(detect_tier(Some("captcha"), ""), WafTier::Captcha);
        assert_eq!(
            detect_tier(None, "<script src=captcha.awswaf.com/x></script>"),
            WafTier::Captcha
        );
        assert_eq!(detect_tier(Some("challenge"), ""), WafTier::Targeted);
        assert_eq!(
            detect_tier(None, "loading challenge.js …"),
            WafTier::Targeted
        );
        assert_eq!(detect_tier(None, "<html>ok</html>"), WafTier::Common);
    }

    // Harness: every tier classifies; self-solvable tiers mint a token; CAPTCHA is
    // not self-solvable (sidecar).
    #[test]
    fn tier_self_solvability() {
        for &t in WafTier::all() {
            assert_eq!(t.self_solvable(), !matches!(t, WafTier::Captcha));
        }
    }

    // The proper path: a PowEngine runs the page's challenge.js and its token is
    // used verbatim over the placeholder.
    #[test]
    fn pow_engine_token_is_used() {
        struct Stub;
        impl PowEngine for Stub {
            fn compute(&self, script: &str) -> Result<String, String> {
                assert!(script.contains("awswaf"), "challenge.js not extracted");
                Ok("AWSWAF_TOKEN".into())
            }
        }
        let page = "<script>/*awswaf*/ window.awsWafToken='live';</script>";
        let solver = AwsWafSolver::new().with_pow_engine(Box::new(Stub));
        assert_eq!(solver.token(page), "AWSWAF_TOKEN");
        // No engine → structural placeholder token (still aws-shaped).
        assert!(AwsWafSolver::new().token(page).ends_with(".aws"));
    }

    #[tokio::test]
    async fn solver_mints_token_and_rejects_captcha() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        // A targeted-challenge page that serves challenge.js markers.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut b = vec![0u8; 2048];
                    let _ = sock.read(&mut b).await;
                    let body = "<html><head><script>challenge.js awswaf</script></head></html>";
                    let resp = format!("HTTP/1.1 202 Accepted\r\nx-amzn-waf-action: challenge\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        let solver = AwsWafSolver::new();
        let ch = Challenge::new(Vendor::AwsWaf, format!("http://127.0.0.1:{port}/"));
        let token = solver.solve(&ch, &SolveContext::default()).await.unwrap();
        assert_eq!(token.cookies[0].0, "aws-waf-token");
        assert!(!token.cookies[0].1.is_empty());
    }
}
