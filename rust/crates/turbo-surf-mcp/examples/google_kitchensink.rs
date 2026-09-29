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
//   3. web_search  — the native (browserless) SERP fetch path; report enablejs-shell vs real SERP.
use serde_json::json;
use turbo_surf_mcp::{call_tool, Session};

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

    // 3) native BROWSERLESS google SERP fetch (engine:google, no forced browser sidecar).
    //    This is the real google test — the default engine is duckduckgo, so we MUST name google.
    println!("\n[3] web_search engine=google (native browserless — no sidecar)");
    match call_tool(
        &mut s,
        "web_search",
        &json!({ "query": query, "engine": "google", "browser": false }),
    )
    .await
    {
        Ok(v) => {
            let n = v.as_array().map(|a| a.len()).unwrap_or(0);
            println!("    google results: {n}");
            println!("    {}", short(&v.to_string(), 500));
        }
        Err(e) => println!("    google verdict: {e}"),
    }

    // 4) control: duckduckgo (not botguarded) — confirms the pipeline itself works.
    println!("\n[4] web_search engine=duckduckgo (control)");
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
