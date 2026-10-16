//! Reusable native half of harness/detection/run.mjs. Fresh jar, no sidecar/cache.
use serde_json::{json, Value};
use std::{fs, path::Path};
use turbo_surf_core::{
    cookies::CookieJar,
    fingerprint,
    net::{fetch_html, FetchOptions},
    request_context::{ClientHints, RequestContext},
};
use turbo_surf_render::PageSession;

const SNAPSHOT: &str = r#"({
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
    const control=document.querySelector('textarea[name=q],input[name=q]');
    return {activeTag:document.activeElement?.tagName||null,
      activeName:document.activeElement?.getAttribute('name')||null,
      value:control?.value??null,disabled:control?.disabled??null,
      readOnly:control?.readOnly??null,consentPresent:!!document.querySelector('#W0wltc')};
  })(),
  readyState:document.readyState,
  nav: (() => {
    const g=globalThis[Symbol.for('ts')];
    const v=g && (g.rscNav || g.nav);
    if(v) return new URL(v,location.href).href;
    return location.href;
  })()
})"#;

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
    selector: &str,
) -> Result<Value, String> {
    let mut stable = None;
    for _ in 0..3 {
        let before = page.navigation_snapshot().0;
        let raw = page.eval_json_expression(SNAPSHOT).await?;
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
    let mut trace = json!({"runtime":"turbo-surf","version":turbo_surf_core::VERSION,
        "instrumented":config["interfaces"].as_bool().unwrap_or(false), "config":config,
        "nativeDebugger":{"enabled":config["debugger"].as_bool().unwrap_or(false),"directory":"native-js","childContexts":turbo_surf_render::stable_window_proxy()},
        "features":build_info["features"],"engine":build_info["engine"],
        "pages":[],"errors":[],"probes":[],"captureComplete":false});
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
    json!({"version":turbo_surf_core::VERSION,
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
    let _session = turbo_surf_mcp::Session::new();
    let profile = fingerprint::default_profile();
    let hints = ClientHints::default();
    let storage = turbo_surf_render::BrowserStorage::default();
    let budget = page_budget(config)?;
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
        let mut page = PageSession::open_with_request_state(
            config["fixtureHtml"]
                .as_str()
                .unwrap_or("<!doctype html><html><head></head><body></body></html>"),
            url,
            "",
            &profile.user_agent,
            budget,
            &init,
            hints.clone(),
        )
        .await?;
        trace["pages"]
            .as_array_mut()
            .unwrap()
            .push(snapshot(&mut page, out, "probe", selector).await?);
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
    let mut page = PageSession::open_with_document_response(
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
    )
    .await?;
    trace["pages"]
        .as_array_mut()
        .unwrap()
        .push(snapshot(&mut page, out, "home", selector).await?);
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
            let wait = config["waitMs"].as_u64().unwrap_or(2500);
            match page
                .input(&json!([{ "click":dismiss },{ "wait":wait }]))
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
                .push(snapshot(&mut page, out, "dismiss", selector).await?);
            checkpoint(out, trace)?;
        }
    }
    let native_before_input = page.navigation_snapshot().0;
    let steps = config["steps"].as_array().ok_or("steps array required")?;
    if !steps.is_empty() {
        if let Err(error) = page.input(&config["steps"]).await {
            trace["pages"]
                .as_array_mut()
                .unwrap()
                .push(json!({"phase":"interaction-error","error":error,"cookies":[]}));
            checkpoint(out, trace)?;
            return Err("Search input failed; navigation was not attempted".into());
        }
        trace["pages"]
            .as_array_mut()
            .unwrap()
            .push(snapshot(&mut page, out, "interaction", selector).await?);
        checkpoint(out, trace)?;
    }
    let (native_after_input, installed_url, _) = page.navigation_snapshot();
    let native_form =
        native_after_input > native_before_input || (steps.is_empty() && native_after_input > 0);
    if native_form {
        trace["inputSource"] = json!("native-host");
        trace["navigationSource"] = json!("native-form");
        trace["formFallback"] = Value::Null;
        let row = snapshot(&mut page, out, "native-final", selector).await?;
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
                context: Some(RequestContext::navigation(Some(referer.clone()), hop == 0)),
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
        let next = PageSession::open_with_document_response(
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
        let row = snapshot(&mut next, out, &format!("navigation-{hop}"), selector).await?;
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
        next.close();
    }
    trace["probes"] = json!(probes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
