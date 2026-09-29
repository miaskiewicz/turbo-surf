// Run the shared fingerprint collector in the turbo-surf render isolate and print its
// JSON snapshot — the turbo-surf side of the Chrome-vs-turbo-surf detection differential.
// Session::new() installs the process-global render hooks (raster/measure/webgl); the
// collector's trailing expression (a JSON string) is what run_with_dom returns.
//
//   cargo run -p turbo-surf-mcp --example fp_snapshot -- /path/to/collector.js
//   cargo run -p turbo-surf-mcp --features gpu-metal --example fp_snapshot -- collector.js
//
// Add `--render` to run through the ASYNC render path (render_page) instead of the sync eval
// runtime (run_with_dom). The GPU WebGL bridge + the full page-lifecycle only run on the render
// path — so any probe that touches WebGL readPixels / rAF / load events must use `--render`.
use std::fs;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let render = args.iter().any(|a| a == "--render");
    let sync_render = args.iter().any(|a| a == "--sync");
    let path = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .expect("usage: fp_snapshot [--render] <collector.js>");
    let collector = fs::read_to_string(path).expect("read collector");
    // Side effect: installs set_measure_fn / set_raster_fn (+ set_webgl_fn under gpu-metal).
    let _s = turbo_surf_mcp::Session::new();
    if sync_render {
        // Sync render path (render_html) — the one the passing GPU-bridge unit test uses.
        let script =
            format!("document.body.setAttribute('data-fp', String((function(){{ return ({collector}); }})()));");
        match turbo_surf_render::render_html("<body></body>", &script) {
            Ok(dom) => {
                let val = dom
                    .split("data-fp=\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next())
                    .unwrap_or("")
                    .replace("&quot;", "\"")
                    .replace("&amp;", "&");
                println!("{val}");
            }
            Err(e) => {
                eprintln!("render error: {e}");
                std::process::exit(1);
            }
        }
    } else if render {
        // Stash the collector's JSON result in a body attribute, render (drives the event loop +
        // GPU bridge), then extract it from the serialized DOM.
        let script =
            format!("document.body.setAttribute('data-fp', String((function(){{ return ({collector}); }})()));");
        match turbo_surf_render::render_page("<body></body>", "https://probe.test/", &script).await
        {
            Ok(dom) => {
                let val = dom
                    .split("data-fp=\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next())
                    .unwrap_or("")
                    .replace("&quot;", "\"")
                    .replace("&amp;", "&");
                println!("{val}");
            }
            Err(e) => {
                eprintln!("render error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        let html = "<html><head></head><body></body></html>";
        match turbo_surf_render::run_with_dom(html, &collector) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("collector error: {e}");
                std::process::exit(1);
            }
        }
    }
}
