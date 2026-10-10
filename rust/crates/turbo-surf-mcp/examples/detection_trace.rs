//! Reusable native half of harness/detection/run.mjs. Fresh jar, no sidecar/cache.
use serde_json::{json, Value};
use std::{fs, path::Path};
use turbo_surf_core::{
    cookies::CookieJar,
    fingerprint,
    net::{fetch_html, FetchOptions},
    request_context::{ClientHints, RequestContext},
};
use turbo_surf_render::{PageSession, HUMAN_INPUT_JS};

const SNAPSHOT: &str = r#"globalThis.__RESULT = JSON.stringify({
  url: location.href,
  interfaces: globalThis[Symbol.for('turbo-surf.interface-trace')]?.snapshot() || null,
  navigator: {userAgent:navigator.userAgent,platform:navigator.platform,
    brands:navigator.userAgentData?.brands,hardwareConcurrency:navigator.hardwareConcurrency},
  nav: (() => {
    const g=globalThis[Symbol.for('ts')];
    const v=g && (g.rscNav || g.nav);
    if(v) return new URL(v,location.href).href;
    return location.href;
  })()
});"#;

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

async fn snapshot(
    page: &mut PageSession,
    out: &Path,
    phase: &str,
    selector: &str,
) -> Result<Value, String> {
    let raw = page.eval(SNAPSHOT).await?;
    let mut row: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let html = page.serialize();
    save(&out.join(format!("{phase}.html")), &html).map_err(|e| e.to_string())?;
    row["phase"] = json!(phase);
    row["bytes"] = json!(html.len());
    row["results"] = json!(turbo_dom_parser::rtdom::Tree::parse(&html)
        .query_selector_all(selector)
        .len());
    row["cookies"] = serde_json::from_str::<Value>(&page.cookies()).unwrap_or(json!([]));
    Ok(row)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let config: Value = serde_json::from_str(&fs::read_to_string(
        args.get(1).ok_or("config path required")?,
    )?)?;
    let out = Path::new(args.get(2).ok_or("output directory required")?);
    fs::create_dir_all(out)?;
    std::env::set_var("TURBO_SURF_HTTP_TRACE", out.join("network.jsonl"));
    let mut trace = json!({"runtime":"turbo-surf","version":turbo_surf_core::VERSION,
        "instrumented":config["interfaces"].as_bool().unwrap_or(false), "config":config,
        "features":{"impersonate":cfg!(feature="impersonate"),"trustAnchors":cfg!(feature="trust-anchors"),
            "gpuMetal":cfg!(feature="gpu-metal"),"coretext":cfg!(feature="coretext")},
        "pages":[],"errors":[],"probes":[]});
    let status = capture(&config, out, &mut trace).await;
    if let Err(e) = &status {
        trace["errors"]
            .as_array_mut()
            .unwrap()
            .push(json!(e.to_string()));
    }
    save(&out.join("trace.json"), serde_json::to_vec_pretty(&trace)?)?;
    println!(
        "{}",
        json!({"out":out,"features":trace["features"],"pages":trace["pages"].as_array().map(Vec::len),"errors":trace["errors"]})
    );
    status
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
    turbo_surf_render::set_fingerprint(
        &json!({
            "chromeMajor":profile.chrome_major,"fullVersion":profile.full_version,
            "platform":profile.nav_platform,"languages":profile.languages,
            "hardwareConcurrency":profile.hardware_concurrency,"deviceMemory":profile.device_memory,
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
            "<html><head></head><body></body></html>",
            "https://probe.test/",
            "",
            &profile.user_agent,
            20_000,
            &init,
            hints.clone(),
        )
        .await?;
        trace["pages"]
            .as_array_mut()
            .unwrap()
            .push(snapshot(&mut page, out, "probe", selector).await?);
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
    let mut page = PageSession::open_with_request_state(
        &response.html,
        &response.final_url,
        &jar.storage_state(),
        &profile.user_agent,
        20_000,
        &init,
        hints.clone(),
    )
    .await?;
    let pages = trace["pages"].as_array_mut().unwrap();
    pages.push(snapshot(&mut page, out, "home", selector).await?);
    page.eval(HUMAN_INPUT_JS).await?;
    let steps = config["steps"].as_array().ok_or("steps array required")?;
    if !steps.is_empty() {
        let steps = serde_json::to_string(steps)?;
        let driver = format!(
            r#"(() => {{
          const q=s=>{{const el=document.querySelector(s);if(!el)throw new Error('Missing selector '+s);return el;}};
          const steps={steps}.map(raw=>{{
          let st=raw;
          if(raw.click) st={{click:q(raw.click)}};
          if(raw.focus) st={{focus:q(raw.focus)}};
          if(raw.move) {{ const el=q(raw.move.selector),r=el.getBoundingClientRect();st={{move:{{el,toX:r.left+r.width/2,toY:r.top+r.height/2}}}}; }}
          return st; }});
          globalThis.__hi.sequence(document,steps,{{startDelay:400,startJitter:0}}).then(()=>{{
            globalThis.__RESULT='done';
          }});
        }})();"#
        );
        if let Err(e) = page.eval(&driver).await {
            pages.push(json!({"phase":"interaction-error","error":e,"cookies":[]}));
        }
        pages.push(snapshot(&mut page, out, "interaction", selector).await?);
    }
    // Follow the actual JS navigation; if the DOM's Enter default is absent, record the
    // serialized GET form fallback separately rather than claiming the JS navigated.
    let raw = page
        .eval(
            r#"globalThis.__RESULT=JSON.stringify((() => {
      const f=document.querySelector('form'); if(!f)return null;
      const u=new URL(f.getAttribute('action')||location.pathname,location.href);
      if((f.getAttribute('method')||'get').toLowerCase()!=='get')return null;
      for(const el of f.querySelectorAll('input[name],textarea[name],select[name]')){
        const type=(el.getAttribute('type')||'').toLowerCase();
        if(el.disabled||['submit','button','reset','file'].includes(type)||
          (['checkbox','radio'].includes(type)&&!el.checked))continue;
        u.searchParams.append(el.getAttribute('name'),el.value??'');
      } return u.href;
    })());"#,
        )
        .await?;
    let fallback: Value = serde_json::from_str(&raw)?;
    let mut target = pages
        .last()
        .and_then(|p| p["nav"].as_str())
        .unwrap_or(&response.final_url)
        .to_string();
    let mut nav_source = "script";
    if !steps.is_empty() && target == response.final_url {
        if let Some(u) = fallback.as_str().filter(|u| *u != response.final_url) {
            target = u.into();
            nav_source = "form-fallback";
        }
    }
    if target == response.final_url {
        probes.extend(collect_probes(&mut page, config).await?);
    }
    jar = CookieJar::from_storage_state(&page.cookies());
    page.close();
    let mut referer = response.final_url.clone();
    for hop in 0..5 {
        if target == referer {
            break;
        }
        let search = fetch_html(
            &target,
            FetchOptions {
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
        let next = PageSession::open_with_request_state(
            &search.html,
            &search.final_url,
            &jar.storage_state(),
            &profile.user_agent,
            20_000,
            &init,
            hints.clone(),
        )
        .await;
        let mut next = match next {
            Ok(p) => p,
            Err(e) => {
                pages.push(json!({"phase":format!("navigation-{hop}-error"),"url":search.final_url,
                    "status":search.status,"error":e,"bytes":search.html.len(),
                    "results":turbo_dom_parser::rtdom::Tree::parse(&search.html).query_selector_all(selector).len(),
                    "cookies":[]}));
                break;
            }
        };
        let mut row = snapshot(&mut next, out, &format!("navigation-{hop}"), selector).await?;
        row["status"] = json!(search.status);
        target = row["nav"].as_str().unwrap_or(&search.final_url).to_string();
        referer = search.final_url;
        jar = CookieJar::from_storage_state(&next.cookies());
        pages.push(row);
        // Collectors run after the verdict snapshot, on the final page only. Their
        // observations are never mixed into the recorded page interface snapshot.
        if target == referer {
            probes.extend(collect_probes(&mut next, config).await?);
        }
        next.close();
    }
    trace["navigationSource"] = json!(nav_source);
    trace["formFallback"] = fallback;
    trace["probes"] = json!(probes);
    Ok(())
}

async fn collect_probes(
    page: &mut PageSession,
    config: &Value,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut probes = Vec::new();
    for path in config["probes"].as_array().into_iter().flatten() {
        let path = path.as_str().ok_or("probe path must be string")?;
        let source = fs::read_to_string(path)?;
        let script = format!(
            "globalThis.__RESULT=({});",
            source.trim().trim_end_matches(';')
        );
        let probe = match page.eval(&script).await {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(result) => json!({"path":path,"result":result}),
                Err(e) => json!({"path":path,"error":e.to_string()}),
            },
            Err(e) => json!({"path":path,"error":e}),
        };
        probes.push(probe);
    }
    Ok(probes)
}
