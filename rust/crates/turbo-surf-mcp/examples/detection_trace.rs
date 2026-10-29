//! Reusable native half of harness/detection/run.mjs. Fresh jar, no sidecar/cache.
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use turbo_surf_core::{
    cookies::CookieJar,
    fingerprint,
    net::{fetch_html, FetchOptions},
    request_context::{ClientHints, RequestContext},
};
use turbo_surf_render::{PageSession, PageSessionOptions, WindowTimerMode};

// Host sleeps give delayed HTTP futures a turn without advancing future window timers.
async fn wait_until(page: &mut PageSession, deadline: Instant) -> Result<(), String> {
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        tokio::time::sleep(remaining.min(Duration::from_millis(25))).await;
        page.eval("void 0").await?;
    }
    Ok(())
}

async fn real_wait(page: &mut PageSession, milliseconds: u64) -> Result<(), String> {
    wait_until(page, Instant::now() + Duration::from_millis(milliseconds)).await
}

fn harness_milliseconds(config: &Value, key: &str, default: u64) -> Result<u64, String> {
    let Some(value) = config.get(key) else {
        return Ok(default);
    };
    value
        .as_u64()
        .filter(|value| *value <= 60_000)
        .ok_or_else(|| format!("{key} must be an integer from 0 to 60000 milliseconds"))
}

async fn input_step(page: &mut PageSession, step: &Value, cadence_ms: u64) -> Result<(), String> {
    if let Some(wait) = step.get("wait") {
        let milliseconds = harness_milliseconds(&json!({"wait":wait}), "wait", 0)?;
        return real_wait(page, milliseconds).await;
    }
    if let Some(text) = step.get("type") {
        let text = text.as_str().ok_or("type must be a string")?;
        for character in text.chars() {
            let deadline = Instant::now() + Duration::from_millis(cadence_ms);
            page.input(&json!([{ "type":character.to_string() }]))
                .await?;
            wait_until(page, deadline).await?;
        }
        return Ok(());
    }
    page.input(&json!([step])).await
}

async fn input_with_real_waits(
    page: &mut PageSession,
    steps: &Value,
    cadence_ms: u64,
) -> Result<(), String> {
    for step in steps.as_array().ok_or("steps array required")? {
        input_step(page, step, cadence_ms).await?;
    }
    Ok(())
}

async fn wait_for_readiness(page: &mut PageSession, config: &Value) -> Result<(), String> {
    let Some(selector) = config["readySelector"].as_str().filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let timeout = harness_milliseconds(config, "readyTimeoutMs", 5000)?;
    let deadline = Instant::now() + Duration::from_millis(timeout);
    let script = format!(
        "!!document.querySelector({})",
        serde_json::to_string(selector).map_err(|error| error.to_string())?
    );
    loop {
        let raw = page.eval_json_expression(&script).await?;
        if serde_json::from_str::<bool>(&raw).map_err(|error| error.to_string())? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Home readiness selector was absent after {timeout}ms"
            ));
        }
        real_wait(page, 25).await?;
    }
}

fn host_milliseconds() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

async fn capture_dwell(
    page: &mut PageSession,
    trace: &mut Value,
    out: &Path,
    stage: &str,
    milliseconds: u64,
) -> Result<(), String> {
    let index = trace["dwell"].as_array().unwrap().len();
    trace["dwell"].as_array_mut().unwrap().push(json!({
        "stage":stage,"requestedMs":milliseconds,"startedMs":host_milliseconds(),"completed":false
    }));
    checkpoint(out, trace).map_err(|error| error.to_string())?;
    trace["dwell"][index]["startedMs"] = json!(host_milliseconds());
    let result = real_wait(page, milliseconds).await;
    trace["dwell"][index]["finishedMs"] = json!(host_milliseconds());
    trace["dwell"][index]["completed"] = json!(result.is_ok());
    checkpoint(out, trace).map_err(|error| error.to_string())?;
    result
}

struct ObservationWindow {
    started: Instant,
    duration: Duration,
}

impl ObservationWindow {
    fn start(milliseconds: u64, index: usize, trace: &mut Value) -> Self {
        let started_ms = host_milliseconds();
        trace["observation"] = json!({"policy":"fixed-deadline-before-enter",
            "source":"host-monotonic-clock","triggerStepIndex":index,
            "requestedMs":milliseconds,"startedMs":started_ms,
            "deadlineMs":started_ms + u128::from(milliseconds),"completed":false});
        Self {
            started: Instant::now(),
            duration: Duration::from_millis(milliseconds),
        }
    }

    fn remaining(&self) -> Duration {
        self.duration.saturating_sub(self.started.elapsed())
    }

    fn finish(&self, trace: &mut Value, completed: bool) {
        let elapsed = self.started.elapsed().as_secs_f64() * 1000.0;
        let requested = self.duration.as_secs_f64() * 1000.0;
        trace["observation"]["finishedMs"] = json!(host_milliseconds());
        trace["observation"]["elapsedMs"] = json!(elapsed);
        trace["observation"]["overrunMs"] = json!((elapsed - requested).max(0.0));
        trace["observation"]["completed"] = json!(completed);
    }
}

fn realtime_options() -> PageSessionOptions {
    PageSessionOptions {
        window_timer_mode: WindowTimerMode::RealTime,
        ..Default::default()
    }
}

async fn guarded_input_step(
    page: &mut PageSession,
    step: &Value,
    cadence_ms: u64,
    selector: Option<&str>,
    guarded: &mut bool,
) -> Result<(), String> {
    if step["press"] == "Enter" && *guarded {
        *guarded = false;
        return page
            .input(&json!([
                {"inputGuard":"verify"}, {"inputGuard":"clear"}, step
            ]))
            .await;
    }
    if let Some(text) = step["type"].as_str() {
        page.input(&json!([{"inputGuard":{"begin":text,"selector":selector}}]))
            .await?;
        *guarded = true;
        input_step(page, step, cadence_ms).await?;
        return page.input(&json!([{"inputGuard":"verify"}])).await;
    }
    input_step(page, step, cadence_ms).await
}

async fn capture_input_steps(
    page: &mut PageSession,
    config: &Value,
    out: &Path,
    trace: &mut Value,
    cadence_ms: u64,
    observation: &mut Option<ObservationWindow>,
) -> Result<(), String> {
    let result =
        capture_input_steps_active(page, config, out, trace, cadence_ms, observation).await;
    let cleanup = page.input(&json!([{"inputGuard":"clear"}])).await;
    result.and(cleanup)
}

async fn capture_input_steps_active(
    page: &mut PageSession,
    config: &Value,
    out: &Path,
    trace: &mut Value,
    cadence_ms: u64,
    observation: &mut Option<ObservationWindow>,
) -> Result<(), String> {
    let steps = config["steps"].as_array().ok_or("steps array required")?;
    let trigger = steps.iter().rposition(|step| step["press"] == "Enter");
    let observation_ms = harness_milliseconds(
        config,
        "observationMs",
        harness_milliseconds(config, "waitMs", 2500)?,
    )?;
    let mut guarded = false;
    let mut focus_selector = None;
    for (index, step) in steps.iter().enumerate() {
        if Some(index) == trigger {
            *observation = Some(ObservationWindow::start(observation_ms, index, trace));
        }
        let started = host_milliseconds();
        let result = guarded_input_step(page, step, cadence_ms, focus_selector, &mut guarded).await;
        if result.is_ok() {
            if let Some(selector) = step["focus"].as_str() {
                focus_selector = Some(selector);
            }
        }
        let action = step
            .as_object()
            .and_then(|value| value.keys().next())
            .map(String::as_str)
            .unwrap_or("unknown");
        trace["actions"].as_array_mut().unwrap().push(json!({
            "index":index,"action":action,"completed":result.is_ok(),
            "startedMs":started,"finishedMs":host_milliseconds()
        }));
        if let Err(error) = result {
            if let Some(window) = observation {
                window.finish(trace, false);
            }
            trace["pages"].as_array_mut().unwrap().push(json!({
                "phase":"interaction-error","stepIndex":index,"error":error,"cookies":[]
            }));
            checkpoint(out, trace).map_err(|error| error.to_string())?;
            return Err("Search input failed; remaining steps were not attempted".into());
        }
        trace["pages"]
            .as_array_mut()
            .unwrap()
            .push(snapshot(page, out, &format!("step-{index}"), config).await?);
        checkpoint(out, trace).map_err(|error| error.to_string())?;
    }
    Ok(())
}

const SNAPSHOT: &str = concat!(
    r#"({
  url: location.href,
  interfaces: globalThis[Symbol.for('turbo-surf.interface-trace')]?.snapshot() || null,
  navigator: {userAgent:navigator.userAgent,platform:navigator.platform,
    brands:navigator.userAgentData?.brands,hardwareConcurrency:navigator.hardwareConcurrency},
  environment: {
    webdriver:navigator.webdriver,userAgent:navigator.userAgent,
    languages:navigator.languages,language:navigator.language,
    locale:Intl.DateTimeFormat().resolvedOptions().locale,
    timezone:Intl.DateTimeFormat().resolvedOptions().timeZone,timeZoneOffset:new Date().getTimezoneOffset(),
    viewport:{width:innerWidth,height:innerHeight},
    outerWindow:{width:outerWidth,height:outerHeight},deviceScaleFactor:devicePixelRatio,
    screen:{width:screen.width,height:screen.height,availWidth:screen.availWidth,availHeight:screen.availHeight}
  },
  browserFixture:globalThis.__browserFixture||null,
  inputState:(() => {
    const captureInputState =
"#,
    include_str!("../../../../harness/detection/input-state.js"),
    r#"
    return captureInputState(document,__TURBO_DISMISS_SELECTOR__);
  })(),
  readyState:document.readyState,
  nav: (() => {
    const g=__tsNs;
    const v=g && (g.rscNav || g.nav);
    if(v) return new URL(v,location.href).href;
    return location.href;
  })()
})"#
);

fn save(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_ref())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents)
    }
}

fn checkpoint(out: &Path, trace: &Value) -> Result<(), Box<dyn std::error::Error>> {
    save(&out.join("trace.json"), serde_json::to_vec_pretty(trace)?)?;
    Ok(())
}

fn page_budget(config: &Value) -> Result<u64, Box<dyn std::error::Error>> {
    let budget = match config.get("pageBudgetMs") {
        Some(value) => value.as_u64().ok_or("pageBudgetMs must be an integer")?,
        None => 20_000,
    };
    if !(1..=120_000).contains(&budget) {
        return Err("pageBudgetMs must be between 1 and 120000".into());
    }
    Ok(budget)
}

async fn snapshot(
    page: &mut PageSession,
    out: &Path,
    phase: &str,
    config: &Value,
) -> Result<Value, String> {
    let selector = config["successSelector"]
        .as_str()
        .unwrap_or("#rso a h3, #search a h3");
    let dismiss = config["dismissSelector"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("#W0wltc");
    let source = SNAPSHOT.replace(
        "__TURBO_DISMISS_SELECTOR__",
        &serde_json::to_string(dismiss).map_err(|error| error.to_string())?,
    );
    let mut stable = None;
    for _ in 0..3 {
        let before = page.navigation_snapshot().0;
        let raw = page.eval_engine_json_expression(&source).await?;
        let installed = page.navigation_snapshot();
        if before == installed.0 {
            stable = Some((raw, installed));
            break;
        }
    }
    let (raw, (count, url, response)) =
        stable.ok_or("Document changed repeatedly during snapshot")?;
    let mut row: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    row["guestUrl"] = row["url"].clone();
    row["url"] = json!(url);
    row["status"] = json!(response.status);
    row["responseReferrer"] = json!(response.referrer);
    row["nativeNavigationCount"] = json!(count);
    let html = page.serialize();
    save(&out.join(format!("{phase}.html")), &html).map_err(|e| e.to_string())?;
    row["phase"] = json!(phase);
    row["bytes"] = json!(html.len());
    row["results"] = json!(turbo_dom_parser::rtdom::Tree::parse(&html)
        .query_selector_all(selector)
        .len());
    row["cookies"] = serde_json::from_str::<Value>(&page.cookies()).unwrap_or(json!([]));
    row["hydration"] = json!(page.hydration_diagnostics());
    row["childFrames"] = json!(page.frame_interface_snapshots());
    Ok(row)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let build_info = build_info();
    if args.get(1).is_some_and(|arg| arg == "--build-info") {
        println!("{build_info}");
        return Ok(());
    }
    let config: Value = serde_json::from_str(&fs::read_to_string(
        args.get(1).ok_or("config path required")?,
    )?)?;
    let out = Path::new(args.get(2).ok_or("output directory required")?);
    fs::create_dir_all(out)?;
    std::env::set_var("TURBO_SURF_HTTP_TRACE", out.join("network.jsonl"));
    if config["debugger"].as_bool().unwrap_or(false) {
        std::env::set_var("TURBO_SURF_JS_TRACE", out.join("native-js"));
    } else {
        std::env::remove_var("TURBO_SURF_JS_TRACE");
    }
    if config["nativeCpuProfile"].as_bool().unwrap_or(false) {
        if !config["debugger"].as_bool().unwrap_or(false) {
            return Err("Native CPU profiling requires debugger inventory".into());
        }
        std::env::set_var("TURBO_SURF_JS_PROFILE", "1");
    } else {
        std::env::remove_var("TURBO_SURF_JS_PROFILE");
    }
    let mut trace = json!({"runtime":"turbo-surf","version":turbo_surf_core::VERSION,
        "instrumented":config["interfaces"].as_bool().unwrap_or(false), "config":config,
        "nativeDebugger":{"enabled":config["debugger"].as_bool().unwrap_or(false),"directory":"native-js","childContexts":turbo_surf_render::stable_window_proxy()},
        "features":build_info["features"],"engine":build_info["engine"],
        "pages":[],"actions":[],"dwell":[],"errors":[],"probes":[],"captureComplete":false});
    checkpoint(out, &trace)?;
    let status = capture(&config, out, &mut trace).await;
    if let Err(e) = &status {
        trace["errors"]
            .as_array_mut()
            .unwrap()
            .push(json!(e.to_string()));
    }
    trace["captureComplete"] = json!(status.is_ok());
    checkpoint(out, &trace)?;
    println!(
        "{}",
        json!({"out":out,"features":trace["features"],"pages":trace["pages"].as_array().map(Vec::len),"errors":trace["errors"]})
    );
    status
}

fn build_info() -> Value {
    let profile = fingerprint::default_profile();
    json!({"version":turbo_surf_core::VERSION,"debugAssertions":cfg!(debug_assertions),
        "features":{"impersonate":cfg!(feature="impersonate"),"trustAnchors":cfg!(feature="trust-anchors"),
            "gpuMetal":cfg!(feature="gpu-metal"),"coretext":cfg!(feature="coretext")},
        "engine":{"v8Version":turbo_surf_render::v8_version(),"stableWindowProxy":turbo_surf_render::stable_window_proxy(),
            "transportChromeMajor":turbo_surf_core::net::transport_chrome_major(),
            "transportBaseChromeMajor":turbo_surf_core::net::transport_base_chrome_major(),
            "reportedChromeMajor":profile.chrome_major,"reportedFullVersion":profile.full_version}})
}

async fn capture(
    config: &Value,
    out: &Path,
    trace: &mut Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Installs every compiled raster/measure/WebGL hook, exactly as the MCP server does.
    turbo_surf_mcp::initialize_render_hooks();
    let profile = fingerprint::default_profile();
    let hints = ClientHints::default();
    let storage = turbo_surf_render::BrowserStorage::default();
    let budget = page_budget(config)?;
    trace["windowTimerMode"] = json!("real-time");
    let wait_ms = harness_milliseconds(config, "waitMs", 2500)?;
    let cadence_ms = harness_milliseconds(config, "inputCadenceMs", 110)?;
    let width = config["viewport"]["width"].as_u64().unwrap_or(1280);
    let height = config["viewport"]["height"].as_u64().unwrap_or(800);
    let screen_width = config["screen"]["width"].as_u64().unwrap_or(1920);
    let screen_height = config["screen"]["height"].as_u64().unwrap_or(1080);
    let outer_width = config["outerWindow"]["width"].as_u64().unwrap_or(width);
    let outer_height = config["outerWindow"]["height"]
        .as_u64()
        .unwrap_or(height + 88);
    if !config["languages"].is_null() && config["languages"] != json!(profile.languages) {
        return Err(
            "Harness languages must match the native HTTP profile's ordered languages".into(),
        );
    }
    if width == 0
        || height == 0
        || width > outer_width
        || height > outer_height
        || outer_width > screen_width
        || outer_height > screen_height
    {
        return Err("Invalid viewport/window/screen geometry".into());
    }
    turbo_surf_render::set_fingerprint(
        &json!({
            "chromeMajor":profile.chrome_major,"fullVersion":profile.full_version,
            "platform":profile.nav_platform,"languages":profile.languages,
            "hardwareConcurrency":profile.hardware_concurrency,"deviceMemory":profile.device_memory,
            "innerWidth":width,"innerHeight":height,"outerWidth":outer_width,"outerHeight":outer_height,
            "screen":{"width":screen_width,"height":screen_height,"availWidth":screen_width,"availHeight":screen_height},
            "devicePixelRatio":config["deviceScaleFactor"].as_f64().unwrap_or(2.0),
        })
        .to_string(),
    );
    let url = config["url"].as_str().ok_or("url required")?;
    let selector = config["successSelector"]
        .as_str()
        .unwrap_or("#rso a h3, #search a h3");
    let init = if config["interfaces"].as_bool().unwrap_or(false) {
        fs::read_to_string(
            config["interfaceScript"]
                .as_str()
                .ok_or("interfaceScript required")?,
        )?
    } else {
        String::new()
    };
    if config["probeOnly"].as_bool().unwrap_or(false) {
        let mut page = PageSession::open_with_document_response_and_options(
            config["fixtureHtml"]
                .as_str()
                .unwrap_or("<!doctype html><html><head></head><body></body></html>"),
            url,
            "",
            &profile.user_agent,
            budget,
            &init,
            hints.clone(),
            &[],
            storage.clone(),
            turbo_surf_render::DocumentResponseMetadata::default(),
            None,
            realtime_options(),
        )
        .await?;
        trace["pages"]
            .as_array_mut()
            .unwrap()
            .push(snapshot(&mut page, out, "probe", config).await?);
        checkpoint(out, trace)?;
        trace["probes"] = json!(collect_probes(&mut page, config).await?);
        page.close();
        return Ok(());
    }
    let mut probes = Vec::new();
    let mut jar = CookieJar::new();
    let response = fetch_html(
        url,
        FetchOptions {
            allow_non_html: true,
            max_redirects: Some(10),
            jar: Some(&mut jar),
            client_hints: Some(&hints),
            context: Some(RequestContext::navigation(None, true)),
            ..Default::default()
        },
    )
    .await?;
    save(&out.join("home-response.html"), &response.html)?;
    let mut page = PageSession::open_with_document_response_and_options(
        &response.html,
        &response.final_url,
        &jar.storage_state(),
        &profile.user_agent,
        budget,
        &init,
        hints.clone(),
        &response.headers,
        storage.clone(),
        turbo_surf_render::DocumentResponseMetadata {
            status: response.status,
            referrer: String::new(),
        },
        None,
        realtime_options(),
    )
    .await?;
    let readiness_started = host_milliseconds();
    trace["readiness"] = json!({"stage":"home","complete":false,
        "selectorRequested":config["readySelector"].as_str().is_some_and(|value| !value.is_empty()),
        "startedMs":readiness_started});
    checkpoint(out, trace)?;
    if let Err(error) = wait_for_readiness(&mut page, config).await {
        trace["readiness"]["error"] = json!(error);
        trace["readiness"]["finishedMs"] = json!(host_milliseconds());
        checkpoint(out, trace)?;
        trace["pages"]
            .as_array_mut()
            .unwrap()
            .push(snapshot(&mut page, out, "readiness-error", config).await?);
        checkpoint(out, trace)?;
        return Err("Home readiness failed; input was not attempted".into());
    }
    trace["readiness"]["complete"] = json!(true);
    trace["readiness"]["finishedMs"] = json!(host_milliseconds());
    trace["inputTiming"] = json!({"source":"host-wall-clock","minimumCharacterMs":cadence_ms,
        "dwellMs":wait_ms,"perStepSnapshots":true,"windowTimerMode":"real-time"});
    capture_dwell(&mut page, trace, out, "home", wait_ms).await?;
    trace["pages"]
        .as_array_mut()
        .unwrap()
        .push(snapshot(&mut page, out, "home", config).await?);
    checkpoint(out, trace)?;
    if let Some(dismiss) = config["dismissSelector"].as_str().filter(|s| !s.is_empty()) {
        let script = format!(
            "!!document.querySelector({})",
            serde_json::to_string(dismiss)?
        );
        let present = serde_json::from_str::<bool>(&page.eval_json_expression(&script).await?)?;
        trace["dismissal"] = json!({"selector":dismiss,"present":present,"clicked":false});
        checkpoint(out, trace)?;
        if present {
            match input_with_real_waits(
                &mut page,
                &json!([{ "move":{"selector":dismiss} },{ "click":dismiss },{ "wait":wait_ms }]),
                cadence_ms,
            )
            .await
            {
                Ok(()) => trace["dismissal"]["clicked"] = json!(true),
                Err(error) => {
                    trace["dismissal"]["error"] = json!(error);
                    checkpoint(out, trace)?;
                    return Err("Consent dismissal input failed; search was not attempted".into());
                }
            }
            trace["pages"]
                .as_array_mut()
                .unwrap()
                .push(snapshot(&mut page, out, "dismiss", config).await?);
            checkpoint(out, trace)?;
        }
    }
    let native_before_input = page.navigation_snapshot().0;
    let steps = config["steps"].as_array().ok_or("steps array required")?;
    let mut observation = None;
    capture_input_steps(&mut page, config, out, trace, cadence_ms, &mut observation).await?;
    if let Some(window) = observation {
        let observed: Result<(), Box<dyn std::error::Error>> = async {
            let native_form = page.navigation_snapshot().0 > native_before_input;
            trace["inputSource"] = json!("native-host");
            trace["navigationSource"] = json!(if native_form { "native-form" } else { "none" });
            trace["formFallback"] = Value::Null;
            let mut hops = 0;
            loop {
                if window.remaining().is_zero() {
                    break;
                }
                let installed = page.navigation_snapshot().1;
                let raw = page
                    .eval_engine_json_expression(
                        r#"(() => {
                const state=__tsNs;
                return new URL(state?.rscNav || state?.nav || location.href,location.href).href;
            })()"#,
                    )
                    .await?;
                let target: String = serde_json::from_str(&raw)?;
                if target == installed {
                    wait_until(
                        &mut page,
                        Instant::now() + window.remaining().min(Duration::from_millis(25)),
                    )
                    .await?;
                    continue;
                }
                trace["observation"]["pendingNavigation"] = json!(target);
                if hops == 5 {
                    trace["observation"]["navigationHopLimit"] = json!(true);
                    break;
                }
                let remaining = window.remaining();
                if remaining.is_zero() {
                    break;
                }
                let mut navigation_jar = CookieJar::from_storage_state(&page.cookies());
                let navigation_metadata =
                    std::sync::Mutex::new(turbo_surf_render::DocumentResponseMetadata::default());
                let response_policy =
                    |context: &RequestContext, target: &str, _headers: &[(String, String)]| {
                        *navigation_metadata.lock().unwrap() =
                            turbo_surf_render::DocumentResponseMetadata::from_request(
                                200, context, target,
                            );
                        true
                    };
                let active = page
                    .script_nav_activation_snapshot()
                    .as_ref()
                    .is_some_and(|(url, active)| *active && url == &target);
                let fetch = tokio::time::timeout(
                    remaining,
                    fetch_html(
                        &target,
                        FetchOptions {
                            response_policy: Some(&response_policy),
                            context: Some(RequestContext::navigation(Some(installed), active)),
                            client_hints: Some(&hints),
                            allow_non_html: true,
                            max_redirects: Some(10),
                            jar: Some(&mut navigation_jar),
                            ..Default::default()
                        },
                    ),
                )
                .await;
                let response = match fetch {
                    Ok(response) => response?,
                    Err(_) => {
                        trace["observation"]["navigationWaitTimedOut"] = json!(true);
                        break;
                    }
                };
                save(
                    &out.join(format!("navigation-{hops}-response.html")),
                    &response.html,
                )?;
                let remaining = window.remaining();
                if remaining.is_zero() {
                    trace["observation"]["navigationWaitTimedOut"] = json!(true);
                    break;
                }
                let metadata = turbo_surf_render::DocumentResponseMetadata {
                    status: response.status,
                    referrer: navigation_metadata.lock().unwrap().referrer.clone(),
                };
                let activation = page.navigation_activation_snapshot();
                let next = tokio::time::timeout(
                    remaining,
                    PageSession::open_with_document_response_and_options(
                        &response.html,
                        &response.final_url,
                        &navigation_jar.storage_state(),
                        &profile.user_agent,
                        budget,
                        &init,
                        hints.clone(),
                        &response.headers,
                        storage.clone(),
                        metadata,
                        Some(activation),
                        PageSessionOptions {
                            initial_hydration_budget_ms: Some(
                                remaining.as_millis().max(1).try_into().unwrap_or(budget),
                            ),
                            ..realtime_options()
                        },
                    ),
                )
                .await;
                let next = match next {
                    Ok(next) => next?,
                    Err(_) => {
                        trace["observation"]["navigationWaitTimedOut"] = json!(true);
                        break;
                    }
                };
                let previous = std::mem::replace(&mut page, next);
                previous.close();
                if !native_form {
                    trace["navigationSource"] = json!("script");
                }
                hops += 1;
                trace["observation"]["followedNavigationHops"] = json!(hops);
                trace["observation"]["pendingNavigation"] = Value::Null;
            }
            trace["observation"]["snapshotStartedMs"] = json!(host_milliseconds());
            let snapshot_started = Instant::now();
            let row = snapshot(&mut page, out, "native-final", config).await?;
            trace["observation"]["snapshotElapsedMs"] =
                json!(snapshot_started.elapsed().as_secs_f64() * 1000.0);
            if row["nav"]
                .as_str()
                .is_some_and(|target| target != page.navigation_snapshot().1)
            {
                trace["observation"]["pendingNavigation"] = row["nav"].clone();
            }
            trace["pages"].as_array_mut().unwrap().push(row);
            if config["requireNativeFormNavigation"]
                .as_bool()
                .unwrap_or(false)
                && !native_form
            {
                return Err(
                    "No native form navigation committed; fallback prohibited by configuration"
                        .into(),
                );
            }
            Ok(())
        }
        .await;
        let window_complete = observed.is_ok()
            && window.remaining().is_zero()
            && trace["observation"]["navigationWaitTimedOut"] != true
            && trace["observation"]["navigationHopLimit"] != true
            && trace["observation"]["pendingNavigation"].is_null();
        window.finish(trace, window_complete);
        checkpoint(out, trace)?;
        observed?;
        trace["probes"] = json!(collect_probes(&mut page, config).await?);
        page.close();
        return Ok(());
    }
    capture_dwell(&mut page, trace, out, "search", wait_ms).await?;
    if !steps.is_empty() {
        trace["pages"]
            .as_array_mut()
            .unwrap()
            .push(snapshot(&mut page, out, "interaction", config).await?);
        checkpoint(out, trace)?;
    }
    let (native_after_input, installed_url, _) = page.navigation_snapshot();
    let native_form =
        native_after_input > native_before_input || (steps.is_empty() && native_after_input > 0);
    if native_form {
        trace["inputSource"] = json!("native-host");
        trace["navigationSource"] = json!("native-form");
        trace["formFallback"] = Value::Null;
        let row = snapshot(&mut page, out, "native-final", config).await?;
        let pending_script_navigation = row["nav"]
            .as_str()
            .is_some_and(|target| target != installed_url);
        trace["pages"].as_array_mut().unwrap().push(row);
        checkpoint(out, trace)?;
        if !pending_script_navigation {
            probes.extend(collect_probes(&mut page, config).await?);
            trace["probes"] = json!(probes);
            checkpoint(out, trace)?;
            page.close();
            return Ok(());
        }
    } else if config["requireNativeFormNavigation"]
        .as_bool()
        .unwrap_or(false)
    {
        return Err(
            "No native form navigation committed; fallback prohibited by configuration".into(),
        );
    }
    // Follow the actual JS navigation; if the DOM's Enter default is absent, record the
    // serialized GET form fallback separately rather than claiming the JS navigated.
    let allow_fallback = !config["disableFormFallback"].as_bool().unwrap_or(false);
    let fallback: Value = if native_form || !allow_fallback {
        Value::Null
    } else {
        let raw = page
            .eval_json_expression(
                r#"(() => {
      const f=document.querySelector('form'); if(!f)return null;
      const u=new URL(f.getAttribute('action')||location.pathname,location.href);
      if((f.getAttribute('method')||'get').toLowerCase()!=='get')return null;
      for(const el of f.querySelectorAll('input[name],textarea[name],select[name]')){
        const type=(el.getAttribute('type')||'').toLowerCase();
        if(el.disabled||['submit','button','reset','file'].includes(type)||
          (['checkbox','radio'].includes(type)&&!el.checked))continue;
        u.searchParams.append(el.getAttribute('name'),el.value??'');
      } return u.href;
    })()"#,
            )
            .await?;
        serde_json::from_str(&raw)?
    };
    let mut target = trace["pages"]
        .as_array()
        .and_then(|pages| pages.last())
        .and_then(|p| p["nav"].as_str())
        .unwrap_or(&installed_url)
        .to_string();
    let mut nav_source = if native_form {
        "native-form"
    } else if target != installed_url {
        "script"
    } else {
        "none"
    };
    if !native_form && !steps.is_empty() && target == installed_url {
        if let Some(u) = fallback.as_str().filter(|u| *u != installed_url) {
            target = u.into();
            nav_source = "form-fallback";
        }
    }
    trace["inputSource"] = json!("native-host");
    trace["navigationSource"] = json!(nav_source);
    trace["formFallback"] = fallback;
    checkpoint(out, trace)?;
    if target == installed_url {
        probes.extend(collect_probes(&mut page, config).await?);
    }
    jar = CookieJar::from_storage_state(&page.cookies());
    let mut script_activation = page.script_nav_activation_snapshot();
    let mut activation = page.navigation_activation_snapshot();
    page.close();
    let mut referer = installed_url.clone();
    for hop in 0..5 {
        if target == referer {
            break;
        }
        let navigation_metadata =
            std::sync::Mutex::new(turbo_surf_render::DocumentResponseMetadata::default());
        let response_policy =
            |context: &RequestContext, target: &str, _headers: &[(String, String)]| {
                *navigation_metadata.lock().unwrap() =
                    turbo_surf_render::DocumentResponseMetadata::from_request(200, context, target);
                true
            };
        let search = fetch_html(
            &target,
            FetchOptions {
                response_policy: Some(&response_policy),
                context: Some(RequestContext::navigation(
                    Some(referer.clone()),
                    (nav_source == "form-fallback" && hop == 0)
                        || script_activation
                            .as_ref()
                            .is_some_and(|(url, active)| *active && url == &target),
                )),
                client_hints: Some(&hints),
                allow_non_html: true,
                max_redirects: Some(10),
                jar: Some(&mut jar),
                ..Default::default()
            },
        )
        .await?;
        save(
            &out.join(format!("navigation-{hop}-response.html")),
            &search.html,
        )?;
        let document_response = turbo_surf_render::DocumentResponseMetadata {
            status: search.status,
            referrer: navigation_metadata.lock().unwrap().referrer.clone(),
        };
        let next = PageSession::open_with_document_response_and_options(
            &search.html,
            &search.final_url,
            &jar.storage_state(),
            &profile.user_agent,
            budget,
            &init,
            hints.clone(),
            &search.headers,
            storage.clone(),
            document_response,
            Some(activation),
            realtime_options(),
        )
        .await;
        let mut next = match next {
            Ok(p) => p,
            Err(e) => {
                trace["pages"].as_array_mut().unwrap().push(json!({"phase":format!("navigation-{hop}-error"),"url":search.final_url,
                    "status":search.status,"error":e,"bytes":search.html.len(),
                    "results":turbo_dom_parser::rtdom::Tree::parse(&search.html).query_selector_all(selector).len(),
                    "cookies":[]}));
                checkpoint(out, trace)?;
                break;
            }
        };
        real_wait(&mut next, wait_ms).await?;
        let row = snapshot(&mut next, out, &format!("navigation-{hop}"), config).await?;
        target = row["nav"].as_str().unwrap_or(&search.final_url).to_string();
        referer = next.navigation_snapshot().1;
        jar = CookieJar::from_storage_state(&next.cookies());
        trace["pages"].as_array_mut().unwrap().push(row);
        checkpoint(out, trace)?;
        // Collectors run after the verdict snapshot, on the final page only. Their
        // observations are never mixed into the recorded page interface snapshot.
        if target == referer {
            probes.extend(collect_probes(&mut next, config).await?);
        }
        script_activation = next.script_nav_activation_snapshot();
        activation = next.navigation_activation_snapshot();
        next.close();
    }
    trace["probes"] = json!(probes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE_ENVIRONMENT: [&str; 4] = [
        "TURBO_SURF_JS_TRACE",
        "TURBO_SURF_JS_PROFILE",
        "TURBO_SURF_TZ",
        "TZ",
    ];

    fn capture_uninstrumented_baseline() {
        let out = Path::new("capture");
        fs::create_dir(out).unwrap();
        let config = json!({"url":"https://baseline.invalid/", "probeOnly":true,
            "fixtureHtml":"<!doctype html><script>window.baseline = true</script>",
            "debugger":false,"nativeCpuProfile":false});
        let mut trace = json!({"pages":[],"probes":[]});
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(capture(&config, out, &mut trace))
            .unwrap();
        for key in BASELINE_ENVIRONMENT {
            assert_eq!(std::env::var_os(key), None, "dotenv changed {key}");
        }
        assert_eq!(trace["pages"].as_array().unwrap().len(), 1);
        assert!(!Path::new("unexpected-js-trace").exists());
        assert!(!out.join("native-js").exists());
    }

    fn capture_fixture_directory() -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "turbo-surf-capture-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        directory
    }

    fn dotenv_fixture() -> std::path::PathBuf {
        let directory = capture_fixture_directory();
        fs::write(
            directory.join(".env"),
            "TURBO_SURF_JS_TRACE=unexpected-js-trace\nTURBO_SURF_JS_PROFILE=1\nTURBO_SURF_TZ=Pacific/Honolulu\nTZ=Pacific/Honolulu\n",
        )
        .unwrap();
        directory
    }

    #[test]
    fn baseline_does_not_load_working_directory_dotenv() {
        const CHILD: &str = "TURBO_SURF_DOTENV_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            capture_uninstrumented_baseline();
            return;
        }
        let before = BASELINE_ENVIRONMENT.map(|key| (key, std::env::var_os(key)));
        let directory = dotenv_fixture();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "tests::baseline_does_not_load_working_directory_dotenv",
                "--nocapture",
            ])
            .current_dir(&directory)
            .env(CHILD, "1")
            .env_remove("TURBO_SURF_HTTP_TRACE")
            .env_remove("TURBO_SURF_HTTP_TRACE_BODY");
        for key in BASELINE_ENVIRONMENT {
            command.env_remove(key);
        }
        let output = command.output().unwrap();
        fs::remove_dir_all(directory).unwrap();
        assert!(
            output.status.success(),
            "baseline subprocess failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for (key, value) in before {
            assert_eq!(std::env::var_os(key), value, "parent environment changed");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_effective_typing_checkpoints_the_action_and_never_sends_enter() {
        let mut page = PageSession::open(
            "<form><input id=q name=q></form><script>window.submits=0;document.querySelector('form').onsubmit=e=>{e.preventDefault();submits++};document.querySelector('#q').oninput=e=>{e.target.value=e.target.value.slice(0,2)}</script>",
            "https://baseline.invalid/", "", "", 5000,
        ).await.unwrap();
        let config = json!({"steps":[{"focus":"#q"},{"type":"query"},{"press":"Enter"}],
            "waitMs":0,"observationMs":0});
        let mut trace = json!({"pages":[],"actions":[]});
        let mut observation = None;
        let directory = capture_fixture_directory();
        let result = capture_input_steps(
            &mut page,
            &config,
            &directory,
            &mut trace,
            0,
            &mut observation,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(trace["actions"].as_array().unwrap().len(), 2);
        assert_eq!(trace["actions"][1]["action"], "type");
        assert_eq!(trace["actions"][1]["completed"], false);
        assert_eq!(
            trace["pages"].as_array().unwrap().last().unwrap()["phase"],
            "interaction-error"
        );
        assert_eq!(
            page.eval_json_expression("[submits,document.querySelector('#q').value]")
                .await
                .unwrap(),
            "[0,\"qu\"]"
        );
        let persisted: Value =
            serde_json::from_str(&fs::read_to_string(directory.join("trace.json")).unwrap())
                .unwrap();
        assert_eq!(persisted["actions"], trace["actions"]);
        let released = page
            .input(&json!([{"inputGuard":"verify"}]))
            .await
            .unwrap_err();
        assert!(released.contains("No retained input control"), "{released}");
        page.close();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn hydration_budget_has_bounded_explicit_configuration() {
        assert_eq!(page_budget(&json!({})).unwrap(), 20_000);
        assert_eq!(
            page_budget(&json!({"pageBudgetMs":60_000})).unwrap(),
            60_000
        );
        for value in [
            json!(0),
            json!(120_001),
            json!(-1),
            json!(1.5),
            json!("20000"),
        ] {
            assert!(page_budget(&json!({"pageBudgetMs":value})).is_err());
        }
    }

    #[test]
    fn comparison_waits_and_typing_cadence_are_explicit_bounded_wall_durations() {
        for (key, default) in [
            ("waitMs", 2500),
            ("inputCadenceMs", 110),
            ("readyTimeoutMs", 5000),
            ("observationMs", 2500),
        ] {
            assert_eq!(
                harness_milliseconds(&json!({}), key, default).unwrap(),
                default
            );
            assert_eq!(
                harness_milliseconds(&json!({key:0}), key, default).unwrap(),
                0
            );
            for invalid in [
                json!(null),
                json!(-1),
                json!(1.5),
                json!(60_001),
                json!("110"),
            ] {
                assert!(harness_milliseconds(&json!({key:invalid}), key, default).is_err());
            }
        }
    }
}

async fn collect_probes(
    page: &mut PageSession,
    config: &Value,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut probes = Vec::new();
    for path in config["probes"].as_array().into_iter().flatten() {
        let path = path.as_str().ok_or("probe path must be string")?;
        let source = fs::read_to_string(path)?;
        let script = format!("({})", source.trim().trim_end_matches(';'));
        let probe = match page.eval_json_expression(&script).await {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(Value::String(json)) => match serde_json::from_str::<Value>(&json) {
                    Ok(result) => json!({"path":path,"result":result}),
                    Err(e) => json!({"path":path,"error":e.to_string()}),
                },
                Ok(result) => json!({"path":path,"result":result}),
                Err(e) => json!({"path":path,"error":e.to_string()}),
            },
            Err(e) => json!({"path":path,"error":e}),
        };
        probes.push(probe);
    }
    Ok(probes)
}
