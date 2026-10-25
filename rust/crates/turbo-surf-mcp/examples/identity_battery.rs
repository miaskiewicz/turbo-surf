//! Native half of harness/detection/chrome-identity-battery.mjs: replays the
//! battery's request kinds through turbo-surf's no-JS fetch tier and its JS render
//! tier with the shipped default identity (no fingerprint overrides). The Node
//! collector owns the loopback servers and records the wire; this binary prints
//! one JSON report line (build identity + the render tier's JS identity probe).
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use turbo_surf_core::{
    cookies::CookieJar,
    net::{client_builder_with_trust, fetch_html, FetchOptions},
    request_context::{ClientHints, RequestContext},
};
use turbo_surf_render::{PageSession, PageSessionOptions, WindowTimerMode};

fn options() -> PageSessionOptions {
    PageSessionOptions {
        window_timer_mode: WindowTimerMode::RealTime,
        ..Default::default()
    }
}

// Host sleeps let in-flight page fetches complete without advancing page timers.
async fn settle(page: &mut PageSession, done: &str, budget: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if page.eval_json_expression(done).await? == "true" {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn navigate(
    url: &str,
    hints: &ClientHints,
    jar: &mut CookieJar,
) -> Result<turbo_surf_core::net::FetchResult, String> {
    fetch_html(
        url,
        FetchOptions {
            allow_non_html: true,
            max_redirects: Some(10),
            jar: Some(jar),
            client_hints: Some(hints),
            context: Some(RequestContext::navigation(None, true)),
            ..Default::default()
        },
    )
    .await
    .map_err(|error| error.to_string())
}

async fn render(
    url: &str,
    hints: &ClientHints,
    jar: &mut CookieJar,
    wait: Duration,
) -> Result<PageSession, String> {
    let profile = turbo_surf_core::fingerprint::default_profile();
    let response = navigate(url, hints, jar).await?;
    PageSession::open_with_document_response_and_options(
        &response.html,
        &response.final_url,
        &jar.storage_state(),
        &profile.user_agent,
        wait.as_millis() as u64 + 10_000,
        "",
        hints.clone(),
        &response.headers,
        turbo_surf_render::BrowserStorage::default(),
        turbo_surf_render::DocumentResponseMetadata {
            status: response.status,
            referrer: String::new(),
        },
        None,
        options(),
    )
    .await
}

async fn run(config: &Value, report: &mut Value) -> Result<(), String> {
    let der = std::fs::read(
        config["certificate"]
            .as_str()
            .ok_or("certificate required")?,
    )
    .map_err(|error| error.to_string())?;
    let probe = std::fs::read_to_string(config["probe"].as_str().ok_or("probe required")?)
        .map_err(|error| error.to_string())?;
    let done = config["done"].as_str().ok_or("done expression required")?;
    let errors = config["errors"]
        .as_str()
        .ok_or("errors expression required")?;
    let builder_der = der.clone();
    // Only the loopback fixture certificate is added; identity stays the default.
    let trusted = move || client_builder_with_trust(&builder_der);

    // Tier 1: the no-JS fetch path, one fresh session for the whole sequence so
    // Accept-CH opt-ins persist exactly as a browser profile's would.
    let hints = ClientHints::with_client_builder(trusted.clone());
    let mut jar = CookieJar::new();
    let mut nojs = Vec::new();
    for url in config["nojs"].as_array().ok_or("nojs urls required")? {
        let url = url.as_str().ok_or("nojs url must be a string")?;
        let status = navigate(url, &hints, &mut jar).await.map(|r| r.status);
        nojs.push(json!({"url": url, "status": status.as_ref().ok(), "error": status.err()}));
    }
    report["nojs"] = json!(nojs);

    // Tier 2: the JS render tier, a separate fresh session.
    let wait = Duration::from_millis(config["render"]["waitMs"].as_u64().unwrap_or(8000));
    let hints = ClientHints::with_client_builder(trusted);
    let mut jar = CookieJar::new();
    let page_url = config["render"]["page"]
        .as_str()
        .ok_or("render.page required")?;
    let mut page = render(page_url, &hints, &mut jar, wait).await?;
    let finished = settle(&mut page, done, wait).await?;
    let js: Value = serde_json::from_str(
        &page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await?,
    )
    .map_err(|error| error.to_string())?;
    let page_errors: Value = serde_json::from_str(&page.eval_json_expression(errors).await?)
        .map_err(|error| error.to_string())?;
    let click = page
        .input(&json!([{ "move": { "selector": "#nav-link" } }, { "click": "#nav-link" }]))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Top-level navigation is the embedder's job (as in detection_trace and the
    // MCP server): fetch the admitted target with the activation captured at
    // admission time.
    let followed = match page.script_nav_activation_snapshot() {
        Some((target, active)) if target.ends_with("/k/click-nav") => {
            let installed = page.navigation_snapshot().1;
            let mut navigation_jar = CookieJar::from_storage_state(&page.cookies());
            fetch_html(
                &target,
                FetchOptions {
                    allow_non_html: true,
                    max_redirects: Some(10),
                    jar: Some(&mut navigation_jar),
                    client_hints: Some(&hints),
                    context: Some(RequestContext::navigation(Some(installed), active)),
                    ..Default::default()
                },
            )
            .await
            .map(|response| json!({"target": target, "active": active, "status": response.status}))
            .map_err(|error| error.to_string())
        }
        other => Err(format!(
            "the trusted click admitted no navigation: {other:?}"
        )),
    };
    page.close();
    let hints_url = config["render"]["hints"]
        .as_str()
        .ok_or("render.hints required")?;
    let mut hinted = render(hints_url, &hints, &mut jar, wait).await?;
    let hinted_done = settle(&mut hinted, done, wait).await?;
    hinted.close();
    report["render"] = json!({
        "finished": finished, "hintsFinished": hinted_done,
        "click": click.err(), "followed": followed.unwrap_or_else(|error| json!({"error": error})),
        "js": js, "pageErrors": page_errors,
    });
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    turbo_surf_mcp::initialize_render_hooks();
    let path = std::env::args().nth(1).ok_or("config path required")?;
    let config: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let profile = turbo_surf_core::fingerprint::default_profile();
    let mut report = json!({
        "build": {
            "crate": env!("CARGO_PKG_VERSION"),
            "impersonate": turbo_surf_core::net::transport_chrome_major().is_some(),
            "v8Version": turbo_surf_render::v8_version(),
            "stableWindowProxy": turbo_surf_render::stable_window_proxy(),
            "defaultMajor": turbo_surf_core::browser::DEFAULT_MAJOR,
            "reportedFullVersion": profile.full_version,
            "transportChromeMajor": turbo_surf_core::net::transport_chrome_major(),
            "transportBaseChromeMajor": turbo_surf_core::net::transport_base_chrome_major(),
        },
        "errors": [],
    });
    if let Err(error) = run(&config, &mut report).await {
        report["errors"] = json!([error]);
    }
    println!("{report}");
    Ok(())
}
