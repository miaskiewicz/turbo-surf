//! Solver selection from the environment / `.env`, the solver-chain plumbing
//! (FallbackSolver, solve_if_challenged), the browser-sidecar protocol and the error
//! surface. Only `solver_selection_chain_and_sidecar_protocol` touches the process
//! environment and current directory; nothing else in this binary reads either.
use std::time::Duration;
use turbo_surf_core::challenge::{
    self, BrowserSolver, Challenge, ChallengeSolver, FallbackSolver, PowEngine, RecaptchaEngine,
    SolveContext, SolveError, SolvedToken, Vendor,
};

struct Fixed(&'static str, bool);
#[async_trait::async_trait]
impl ChallengeSolver for Fixed {
    fn name(&self) -> &'static str {
        self.0
    }
    async fn solve(&self, _: &Challenge, _: &SolveContext) -> Result<SolvedToken, SolveError> {
        if self.1 {
            Ok(SolvedToken {
                cookies: vec![("from".into(), self.0.into())],
                headers: Vec::new(),
                ttl: Duration::from_secs(1),
            })
        } else {
            Err(SolveError::Http(format!("{} failed", self.0)))
        }
    }
}

struct NoPow;
impl PowEngine for NoPow {
    fn compute(&self, _: &str) -> Result<String, String> {
        Err("unused".into())
    }
}
struct NoRecaptcha;
#[async_trait::async_trait]
impl RecaptchaEngine for NoRecaptcha {
    async fn execute(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
        Err("unused".into())
    }
}

fn from(t: &SolvedToken) -> &str {
    &t.cookies[0].1
}

fn select(solver: &str) -> Option<&'static str> {
    std::env::set_var("TURBO_SURF_SOLVER", solver);
    challenge::solver_from_env().map(|s| s.name())
}

fn select_pow(solver: &str) -> Option<&'static str> {
    std::env::set_var("TURBO_SURF_SOLVER", solver);
    challenge::solver_from_env_pow(Some(Box::new(NoPow)), Some(Box::new(NoRecaptcha)))
        .map(|s| s.name())
}

async fn sidecar(cmd: &str, ch: &Challenge, ctx: &SolveContext) -> Result<SolvedToken, String> {
    BrowserSolver::new(cmd.into())
        .solve(ch, ctx)
        .await
        .map_err(|e| e.to_string())
}

#[test]
fn solver_selection_chain_and_sidecar_protocol() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    // ---- vendor labels + error messages (the strings callers and logs see) ----
    let labels: Vec<&str> = [
        Vendor::Akamai,
        Vendor::DataDome,
        Vendor::Kasada,
        Vendor::Cloudflare,
        Vendor::AwsWaf,
        Vendor::Recaptcha,
    ]
    .iter()
    .map(|v| v.as_str())
    .collect();
    assert_eq!(
        labels,
        ["akamai", "datadome", "kasada", "cloudflare", "awswaf", "recaptcha"]
    );
    assert_eq!(SolveError::NotConfigured.to_string(), "no solver configured");
    assert_eq!(SolveError::Http("x".into()).to_string(), "solver HTTP error: x");
    assert_eq!(
        SolveError::Parse("y".into()).to_string(),
        "solver response parse error: y"
    );
    assert_eq!(
        SolveError::Unsupported(Vendor::Kasada).to_string(),
        "solver does not support kasada"
    );

    // ---- FallbackSolver: primary wins when it succeeds, fallback on its error ----
    let ch = Challenge {
        vendor: Vendor::Akamai,
        page_url: "https://shop.test/p".into(),
        ..Default::default()
    };
    let ctx = SolveContext {
        user_agent: "UA/9".into(),
        proxy: Some("http://proxy.test:1".into()),
    };
    let ok_first = FallbackSolver::new(Box::new(Fixed("a", true)), Box::new(Fixed("b", true)));
    assert_eq!(ok_first.name(), "fallback");
    assert_eq!(from(&rt.block_on(ok_first.solve(&ch, &ctx)).unwrap()), "a");
    let falls = FallbackSolver::new(Box::new(Fixed("a", false)), Box::new(Fixed("b", true)));
    assert_eq!(from(&rt.block_on(falls.solve(&ch, &ctx)).unwrap()), "b");
    let both = FallbackSolver::new(Box::new(Fixed("a", false)), Box::new(Fixed("b", false)));
    assert_eq!(
        rt.block_on(both.solve(&ch, &ctx)).unwrap_err().to_string(),
        "solver HTTP error: b failed"
    );

    // ---- solve_if_challenged: a normal page is Ok(None); a wall reaches the solver ----
    let none = rt.block_on(challenge::solve_if_challenged(
        &Fixed("s", true),
        "https://x.test/",
        200,
        &[],
        "<html>fine</html>",
        &ctx,
    ));
    assert!(matches!(none, Ok(None)), "{none:?}");
    let walled = rt.block_on(challenge::solve_if_challenged(
        &Fixed("s", true),
        "https://x.test/",
        403,
        &[("CF-Mitigated".to_string(), "challenge".to_string())],
        "<title>Just a moment...</title>",
        &ctx,
    ));
    assert_eq!(from(&walled.unwrap().unwrap()), "s");

    // ---- the browser sidecar: request on stdin, token envelope on stdout ----
    // `cat` echoes the request back; it is not a token envelope, so the parse error
    // quotes it, which shows what the sidecar received.
    let echoed = rt.block_on(sidecar("cat", &ch, &ctx)).unwrap_err();
    assert!(echoed.starts_with("solver response parse error: no token in response: {"));
    for field in [
        r#""url":"https://shop.test/p""#,
        r#""vendor":"akamai""#,
        r#""userAgent":"UA/9""#,
        r#""proxy":"http://proxy.test:1""#,
    ] {
        assert!(echoed.contains(field), "{field} missing from {echoed}");
    }
    let token = rt
        .block_on(sidecar(
            r#"cat >/dev/null; echo '{"cookies":{"_abck":"A","n":1},"headers":{"x-kpsdk-ct":"K"}}'"#,
            &ch,
            &ctx,
        ))
        .unwrap();
    assert_eq!(token.cookies, vec![("_abck".to_string(), "A".to_string())]);
    assert_eq!(token.headers, vec![("x-kpsdk-ct".to_string(), "K".to_string())]);
    assert_eq!(token.ttl, Duration::from_secs(30 * 60));
    // Either half of the envelope alone is a token.
    let cookies_only = rt
        .block_on(sidecar(r#"cat >/dev/null; echo '{"cookies":{"c":"1"}}'"#, &ch, &ctx))
        .unwrap();
    assert_eq!(cookies_only.cookies, vec![("c".to_string(), "1".to_string())]);
    assert!(cookies_only.headers.is_empty());
    let headers_only = rt
        .block_on(sidecar(r#"cat >/dev/null; echo '{"headers":{"h":"2"}}'"#, &ch, &ctx))
        .unwrap();
    assert!(headers_only.cookies.is_empty());
    assert_eq!(
        rt.block_on(sidecar("cat >/dev/null; echo '{}'", &ch, &ctx))
            .unwrap_err(),
        "solver response parse error: no token in response: {}"
    );
    assert!(rt
        .block_on(sidecar("cat >/dev/null; echo not-json", &ch, &ctx))
        .unwrap_err()
        .starts_with("solver response parse error: "));
    assert_eq!(
        rt.block_on(sidecar("cat >/dev/null; echo boom >&2; exit 3", &ch, &ctx))
            .unwrap_err(),
        "solver HTTP error: sidecar exit Some(3): boom\n"
    );

    // ---- env selection (no `.env` in reach: run from an empty directory) ----
    let dir = std::env::temp_dir().join(format!("surf-solver-env-{}", std::process::id()));
    let nested = dir.join("a/b/c");
    std::fs::create_dir_all(&nested).unwrap();
    std::env::set_current_dir(&nested).unwrap();
    for k in [
        "HYPER_API_KEY",
        "SCRAPFLY_API_KEY",
        "TURBO_SURF_SOLVER",
        "TURBO_SURF_BROWSER_CMD",
    ] {
        std::env::remove_var(k);
    }
    // In-house solvers stand alone without a sidecar ...
    assert_eq!(select("akamai"), Some("akamai"));
    assert_eq!(select("Cloudflare"), Some("cloudflare"));
    assert_eq!(select("awswaf"), Some("awswaf"));
    assert_eq!(select("aws"), Some("awswaf"));
    assert_eq!(select("recaptcha"), Some("recaptcha"));
    assert_eq!(select_pow("cloudflare"), Some("cloudflare"));
    assert_eq!(select_pow("aws"), Some("awswaf"));
    assert_eq!(select_pow("recaptcha"), Some("recaptcha"));
    // ... and fall back to the browser sidecar when one is configured (a blank
    // command counts as none).
    std::env::set_var("TURBO_SURF_BROWSER_CMD", "  ");
    assert_eq!(select("akamai"), Some("akamai"));
    std::env::set_var("TURBO_SURF_BROWSER_CMD", "sidecar --json");
    for s in ["akamai", "cloudflare", "awswaf", "recaptcha"] {
        assert_eq!(select(s), Some("fallback"), "{s}");
    }
    std::env::remove_var("TURBO_SURF_BROWSER_CMD");
    // Nothing configured → None. An explicit hyper/scrapfly pick needs its own key;
    // the auto pick prefers hyper.
    assert_eq!(select(""), None);
    std::env::set_var("SCRAPFLY_API_KEY", "sk_real");
    assert_eq!(select("hyper"), None);
    assert_eq!(select("scrapfly"), Some("scrapfly"));
    assert_eq!(select(""), Some("scrapfly"));
    std::env::set_var("HYPER_API_KEY", "hk_real");
    assert_eq!(select("scrapfly"), Some("scrapfly"));
    assert_eq!(select(""), Some("hyper"));
    std::env::remove_var("HYPER_API_KEY");
    std::env::remove_var("SCRAPFLY_API_KEY");

    // `.env` (searched up to three parents) fills unset vars only: comments, blank
    // lines and lines without '=' are ignored, values are trimmed and unquoted.
    std::fs::write(
        dir.join("a/.env"),
        "# COMMENTED_OUT=1\n\n  HYPER_API_KEY = \"hk_real\"  \nNOT_AN_ASSIGNMENT\nSCRAPFLY_API_KEY=from_dotenv\n",
    )
    .unwrap();
    std::env::set_var("SCRAPFLY_API_KEY", "already-set");
    assert_eq!(select("hyper"), Some("hyper"));
    assert_eq!(std::env::var("HYPER_API_KEY").as_deref(), Ok("hk_real"));
    assert_eq!(
        std::env::var("SCRAPFLY_API_KEY").as_deref(),
        Ok("already-set")
    );
    assert!(std::env::var("NOT_AN_ASSIGNMENT").is_err());
    assert!(std::env::var("# COMMENTED_OUT").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn vendor_solvers_surface_transport_errors_and_parse_edges() {
    use turbo_surf_core::{akamai, aws_waf, cloudflare, recaptcha};
    let dead = Challenge {
        page_url: "http://127.0.0.1:1/wall".into(),
        ..Default::default()
    };
    let ctx = SolveContext::default();
    let solvers: Vec<Box<dyn ChallengeSolver>> = vec![
        Box::new(akamai::AkamaiSolver::default()),
        Box::new(cloudflare::CloudflareSolver::default()),
        Box::new(aws_waf::AwsWafSolver::default()),
        Box::new(recaptcha::RecaptchaSolver::default()),
    ];
    for solver in &solvers {
        let err = solver.solve(&dead, &ctx).await.unwrap_err();
        assert!(
            matches!(err, SolveError::Http(_)),
            "{}: {err:?}",
            solver.name()
        );
    }
    assert_eq!(
        [
            aws_waf::WafTier::Common,
            aws_waf::WafTier::Targeted,
            aws_waf::WafTier::Captcha
        ]
        .map(|t| t.label()),
        ["common", "targeted", "captcha"]
    );
    // Site-key extraction skips non-word-start hits, keys without a separator or a
    // quoted value, and a bare `render=explicit`.
    assert_eq!(
        recaptcha::extract_sitekey(
            r#"data-sitekeyx="no" sitekey sitekey= sitekey=bare 'sitekey' : "K1""#
        )
        .as_deref(),
        Some("K1")
    );
    assert_eq!(recaptcha::extract_sitekey("sitekey="), None);
    assert_eq!(
        recaptcha::extract_sitekey("grecaptcha.execute(KEY)"),
        None
    );
    assert_eq!(
        recaptcha::extract_sitekey("grecaptcha.execute('V3KEY', {})").as_deref(),
        Some("V3KEY")
    );
    // Cloudflare fields must be quoted JS strings.
    assert_eq!(
        cloudflare::parse_challenge("window._cf_chl_opt={cRay: 123, cvId: '2'}"),
        None
    );
}
