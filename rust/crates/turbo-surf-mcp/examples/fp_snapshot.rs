// Run the shared fingerprint collector in the turbo-surf render isolate and print its
// JSON snapshot — the turbo-surf side of the Chrome-vs-turbo-surf detection differential.
// Session::new() installs the process-global render hooks (raster/measure/webgl); the
// collector's trailing expression (a JSON string) is what run_with_dom returns.
//
//   cargo run -p turbo-surf-mcp --example fp_snapshot -- /path/to/collector.js
//   cargo run -p turbo-surf-mcp --features gpu-metal --example fp_snapshot -- collector.js
use std::fs;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: fp_snapshot <collector.js>");
    let collector = fs::read_to_string(&path).expect("read collector");
    // Side effect: installs set_measure_fn / set_raster_fn (+ set_webgl_fn under gpu-metal).
    let _s = turbo_surf_mcp::Session::new();
    let html = "<html><head></head><body></body></html>";
    match turbo_surf_render::run_with_dom(html, &collector) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("collector error: {e}");
            std::process::exit(1);
        }
    }
}
