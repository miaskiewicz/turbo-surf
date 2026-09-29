// LIVE browserless google SERP attempt — the full kitchen sink, no sidecar browser.
// Build with `--features gpu-metal` to include the real Apple-GPU canvas + WebGL bridge.
//
//   TURBO_SURF_TRACE=1 cargo run -p turbo-surf-mcp --features gpu-metal \
//       --example google_kitchensink -- "rust lang"
//
// Steps, all through the same pub tool dispatch the MCP server uses:
//   1. probe_mint  — run google's homepage integrity JS to completion in-isolate under the
//      full fidelity globals; report what env it demanded + any cookie it minted browserlessly.
//   2. goto homepage + human_interact google-serp — trusted, entropy-bearing search-box
//      interaction in the render isolate, then keep the hydrated result.
//   3. browserless_google_serp — mint an __Secure-ENID IN-ISOLATE and replay it on a native
//      /search (no sidecar, no Chromium); report whether that ENID is trusted (real SERP vs shell).
//   4. web_search duckduckgo — browserless control (not botguarded) proving the pipeline works.
use serde_json::json;
use turbo_surf_mcp::{browserless_google_serp, call_tool, Session};

fn short(s: &str, n: usize) -> String {
    let s = s.replace('\n', " ");
    if s.len() > n {
        format!("{}…", &s[..n])
    } else {
        s
    }
}

// The render tier's `op_fetch` requires a current-thread tokio runtime (deno_core/deno_unsync),
// the same flavor the MCP stdio binary uses — a multi-thread runtime panics inside op_fetch.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "rust lang".to_string());
    let feat = if cfg!(feature = "gpu-metal") {
        "gpu-metal ON"
    } else {
        "gpu-metal OFF"
    };
    println!("=== live browserless google kitchen sink ({feat}) query={query:?} ===\n");

    let mut s = Session::new();

    // 1) in-isolate browserless mint attempt
    println!("[1] probe_mint https://www.google.com/ (in-isolate BotGuard run)");
    match call_tool(
        &mut s,
        "probe_mint",
        &json!({ "url": "https://www.google.com/" }),
    )
    .await
    {
        Ok(v) => {
            println!(
                "    status      : {}",
                v.get("status").unwrap_or(&json!("?"))
            );
            println!(
                "    scripts_len : {}",
                v.get("scripts_len").unwrap_or(&json!("?"))
            );
            println!(
                "    earned      : {}",
                v.get("earned_cookies").unwrap_or(&json!("?"))
            );
            println!(
                "    minted_enid : {}",
                v.get("minted_enid").unwrap_or(&json!("?"))
            );
            if let Some(g) = v.get("shim_needed") {
                println!("    shim_needed : {}", short(&g.to_string(), 200));
            }
        }
        Err(e) => println!("    ERR: {e}"),
    }

    // 2) goto + human_interact (trusted search-box interaction in the render isolate)
    println!("\n[2] goto homepage + human_interact google-serp routine");
    let _ = call_tool(&mut s, "set_mode", &json!({ "mode": "secure" })).await;
    match call_tool(&mut s, "goto", &json!({ "url": "https://www.google.com/" })).await {
        Ok(_) => {
            let hi = call_tool(
                &mut s,
                "human_interact",
                &json!({ "routine": "google-serp", "params": { "query": query } }),
            )
            .await;
            match hi {
                Ok(v) => {
                    println!(
                        "    completed   : {}",
                        v.get("completed").unwrap_or(&json!("?"))
                    );
                    println!(
                        "    steps       : {}",
                        v.get("steps").unwrap_or(&json!("?"))
                    );
                    // The interaction fills the search box + clicks Search; human_interact then
                    // FOLLOWS the resulting /search navigation natively (carrying the homepage jar).
                    println!(
                        "    navigated   : {}",
                        v.get("navigated").unwrap_or(&json!("?"))
                    );
                    println!(
                        "    navigated_to: {}",
                        v.get("navigated_to")
                            .and_then(|x| x.as_str())
                            .unwrap_or("(none)")
                    );
                    let dom = call_tool(&mut s, "latest_dom", &json!({}))
                        .await
                        .unwrap_or(json!(null));
                    let html = dom.as_str().unwrap_or("");
                    let has_rso = html.contains("id=\"rso\"") || html.contains("id='rso'");
                    let has_h3 = html.contains("<h3");
                    println!(
                        "    hydrated    : len={} #rso={} <h3>={} → {}",
                        html.len(),
                        has_rso,
                        has_h3,
                        if has_rso && has_h3 {
                            "LOOKS LIKE REAL SERP"
                        } else {
                            "no SERP markers"
                        }
                    );
                }
                Err(e) => println!("    human_interact ERR: {e}"),
            }
        }
        Err(e) => println!("    goto ERR: {e}"),
    }

    // 3) THE definitive fully-browserless test: real consent Accept-all handshake → in-isolate
    //    BotGuard → native /search — no sidecar, no Chromium. Reports whether the ENID is TRUSTED.
    println!("\n[3] browserless_google_serp (consent handshake + in-isolate → native /search, NO sidecar)");
    match browserless_google_serp(&query).await {
        Ok(v) => {
            let g = |k: &str| v.get(k).cloned().unwrap_or(json!("?"));
            println!("    consent_hshk : {}", g("consent_handshake"));
            println!("    earned       : {}", g("earned_cookies"));
            println!(
                "    has_aec      : {}  has_nid: {}",
                g("has_aec"),
                g("has_nid")
            );
            println!("    minted_enid  : {}", g("minted_enid"));
            println!("    enid_trusted : {}", g("enid_trusted"));
            println!(
                "    verdict      : {}",
                g("verdict").as_str().unwrap_or("?")
            );
            println!("    body_len     : {}", g("body_len"));
            let n = v
                .get("results")
                .and_then(|r| r.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            println!("    results      : {n}");
            if n > 0 {
                println!("    {}", short(&v["results"].to_string(), 400));
            }
        }
        Err(e) => println!("    ERR: {e}"),
    }

    // 4) control: duckduckgo (not botguarded, browserless) — confirms the pipeline itself works.
    println!("\n[4] web_search engine=duckduckgo (browserless control)");
    match call_tool(
        &mut s,
        "web_search",
        &json!({ "query": query, "engine": "duckduckgo" }),
    )
    .await
    {
        Ok(v) => println!(
            "    ddg results: {}",
            v.as_array().map(|a| a.len()).unwrap_or(0)
        ),
        Err(e) => println!("    ddg verdict: {e}"),
    }

    println!("\n=== done ===");
}
