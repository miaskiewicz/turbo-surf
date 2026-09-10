//! `probe_globals` tests in their own test process. These boot a deno_core
//! runtime (which initializes the V8 platform), so they must NOT share a binary
//! with the `browser_env` standalone-V8 smoke test (the lib unit-test binary).
//! A separate integration-test binary = a separate process = its own one-time
//! V8 init, so the two never collide (same split as `tests/render.rs`).

use turbo_surf_render::probe_globals;

#[test]
fn reports_touched_props_and_shim_gaps() {
    // A script that profiles the browser the way an anti-bot collector would.
    let script = r#"
        const _ = [navigator.userAgent, navigator.platform, navigator.webdriver,
                   navigator.hardwareConcurrency, navigator.languages,
                   navigator.thisDoesNotExist, screen.width, window.chrome];
        navigator.plugins;
        ''
    "#;
    let r = probe_globals("<body></body>", script).unwrap();
    let touched = |t: &str, p: &str| {
        r.accesses
            .iter()
            .any(|a| a.target == t && a.prop == p && a.kind == "get")
    };
    assert!(touched("navigator", "userAgent"));
    assert!(touched("navigator", "webdriver"));
    assert!(touched("navigator", "platform"));
    // The real Chrome profile is present → not a shim gap.
    assert!(!r.shim_needed.iter().any(|s| s == "navigator.userAgent"));
    // The bogus prop returned undefined → flagged as a gap to shim.
    assert!(r
        .shim_needed
        .iter()
        .any(|s| s == "navigator.thisDoesNotExist"));
}

#[test]
fn flags_canvas_fingerprinting() {
    let script = r#"
        const c = document.createElement('canvas');
        const ctx = c.getContext('2d');
        try { c.toDataURL(); } catch (e) {}
        ''
    "#;
    let r = probe_globals("<body></body>", script).unwrap();
    assert!(r
        .accesses
        .iter()
        .any(|a| a.target == "document" && a.prop == "createElement(canvas)"));
    assert!(r
        .accesses
        .iter()
        .any(|a| a.target == "canvas" && a.prop == "getContext"));
}

// Helper: did the probe record a `get` (read) of target.prop?
fn touched(r: &turbo_surf_render::ProbeReport, t: &str, p: &str) -> bool {
    r.accesses
        .iter()
        .any(|a| a.target == t && a.prop == p && a.kind == "get")
}

#[test]
fn instruments_the_window_surface_and_document_beyond_create_element() {
    // The extended recon must capture window-level reads AND document members other
    // than createElement (BotGuard reads readyState/contentType/currentScript, etc.).
    let script = r#"
        const _ = [window.self, window.navigator, window.document, window.performance,
                   document.readyState, document.contentType];
        ''
    "#;
    let r = probe_globals("<body></body>", script).unwrap();
    assert!(touched(&r, "window", "self"));
    assert!(touched(&r, "window", "navigator"));
    assert!(touched(&r, "window", "performance"));
    // document.contentType is backfilled in ENV_BOOTSTRAP → recorded and NOT a gap.
    assert!(touched(&r, "document", "contentType"));
    assert!(touched(&r, "document", "readyState"));
    assert!(!r.shim_needed.iter().any(|s| s == "document.contentType"));
}

#[test]
fn flags_webgl_context_as_a_shim_gap_but_wraps_2d() {
    // getContext('2d') yields a context whose reads register under `ctx:2d`; WebGL
    // returns null in the vendored binding → surfaced as a shim gap to backfill.
    let script = r#"
        const c = document.createElement('canvas');
        const ctx = c.getContext('2d');
        try { ctx.measureText('x'); } catch (e) {}
        const gl = c.getContext('webgl');
        ''
    "#;
    let r = probe_globals("<body></body>", script).unwrap();
    assert!(r
        .accesses
        .iter()
        .any(|a| a.target == "canvas" && a.prop == "getContext(2d)" && a.kind == "call"));
    assert!(r.accesses.iter().any(|a| a.target == "ctx:2d"));
    assert!(r
        .shim_needed
        .iter()
        .any(|s| s == "canvas.getContext(webgl)=>null"));
}

#[test]
fn counts_function_prototype_tostring_anti_tamper_reads() {
    // Anti-tamper `fn.toString()` reads (native-code probes) are counted so recon can
    // quantify how aggressively a VM inspects the function surface.
    let script = r#"
        const f = function foo() {};
        f.toString();
        Array.prototype.push.toString();
        ''
    "#;
    let r = probe_globals("<body></body>", script).unwrap();
    assert!(r
        .accesses
        .iter()
        .any(|a| a.target == "Function.prototype" && a.prop == "toString" && a.kind == "call"));
}

#[test]
fn host_protocol_shims_are_present_not_gaps() {
    // The reCAPTCHA host-protocol backfills (window.postMessage / onmessage /
    // trustedTypes) must read as defined, not surface as shim gaps.
    let script = r#"
        const _ = [typeof window.postMessage, window.onmessage, typeof window.trustedTypes,
                   typeof window.MessageChannel];
        ''
    "#;
    let r = probe_globals("<body></body>", script).unwrap();
    assert!(touched(&r, "window", "postMessage"));
    assert!(!r.shim_needed.iter().any(|s| s == "window.postMessage"));
    // onmessage defaults to null (defined), never undefined.
    assert!(!r.shim_needed.iter().any(|s| s == "window.onmessage"));
    assert!(!r.shim_needed.iter().any(|s| s == "window.trustedTypes"));
}
