//! The render tier's JS identity against the default release's measured Chrome
//! fixture (chrome-identity-<DEFAULT_MAJOR>.json), using the same probe file.
//! Machine-specific fields compare against the profile; genuine gaps are pinned.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

fn read(relative: &str) -> String {
    let path = format!("{}/../../../{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).expect(&path)
}

#[tokio::test(flavor = "current_thread")]
async fn render_navigator_matches_the_measured_chrome_identity() {
    let major = turbo_surf_core::browser::DEFAULT_MAJOR;
    let fixture: Value = serde_json::from_str(&read(&format!(
        "harness/detection/fixtures/chrome-identity-{major}.json"
    )))
    .unwrap();
    let probe = read("harness/detection/chrome-identity-probe.js");
    assert_eq!(
        fixture["collector"]["chrome-identity-probe.js"],
        format!("{:x}", Sha256::digest(&probe)),
        "the fixture was measured with this probe"
    );
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://identity.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    page.close();
    let expected = &fixture["js"];
    let profile = turbo_surf_core::fingerprint::default_profile();
    for key in [
        "userAgent",
        "appVersion",
        "appName",
        "appCodeName",
        "product",
        "productSub",
        "vendor",
        "vendorSub",
        "platform",
        "language",
        "languages",
        "deviceMemory",
        "maxTouchPoints",
        "cookieEnabled",
        "doNotTrack",
        "pdfViewerEnabled",
        "onLine",
        "plugins",
        "mimeTypes",
        "ownKeys",
    ] {
        assert_eq!(
            actual["navigator"][key], expected["navigator"][key],
            "navigator.{key}"
        );
    }
    // Collector artifact: CDP remote debugging makes the measured Chrome report true.
    assert_eq!(expected["navigator"]["webdriver"], true);
    assert_eq!(actual["navigator"]["webdriver"], false);
    for key in ["brands", "mobile", "platform"] {
        assert_eq!(
            actual["userAgentData"][key], expected["userAgentData"][key],
            "{key}"
        );
    }
    let high = &actual["userAgentData"]["high"];
    for key in [
        "bitness",
        "brands",
        "formFactors",
        "fullVersionList",
        "mobile",
        "model",
        "platform",
        "uaFullVersion",
        "wow64",
    ] {
        assert_eq!(
            high[key], expected["userAgentData"]["high"][key],
            "high.{key}"
        );
    }
    // Host-derived values follow the running machine, like Chrome's own.
    assert_eq!(high["architecture"], profile.architecture());
    assert_eq!(high["platformVersion"], profile.platform_version());
    // NavigatorUAData.prototype.toJSON() serialises the low-entropy values (turbo-surf#164).
    assert!(expected["userAgentData"]["json"]["brands"].is_array());
    assert_eq!(
        actual["userAgentData"]["json"],
        expected["userAgentData"]["json"]
    );
}

/// Chrome 156: `navigator.userAgentData` is a NavigatorUAData instance with zero own
/// properties; brands/mobile/platform are brand-checked prototype getters, and
/// getHighEntropyValues/toJSON are native prototype operations (turbo-surf#164).
/// Expected values measured in Chrome 156.0.8078.47 headless shell.
#[tokio::test(flavor = "current_thread")]
async fn user_agent_data_is_a_navigator_ua_data_instance() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://identity.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                r#"(() => {
  const u = navigator.userAgentData, P = NavigatorUAData.prototype;
  const d = k => Object.getOwnPropertyDescriptor(P, k);
  let illegal = "";
  try { d("brands").get.call({}); } catch (e) { illegal = e.constructor.name + ": " + e.message; }
  let ctor = "";
  try { new NavigatorUAData(); } catch (e) { ctor = e.constructor.name + ": " + e.message; }
  try { NavigatorUAData(); } catch (e) { ctor += " | " + e.message; }
  return {
    tag: Object.prototype.toString.call(u),
    proto: Object.getPrototypeOf(u) === P,
    own: Object.getOwnPropertyNames(u),
    keys: Object.getOwnPropertyNames(P),
    getters: ["brands", "mobile", "platform"].map(k => typeof d(k).get + " " + d(k).enumerable + " " + d(k).get.name + " " + String(d(k).get).includes("[native code]")),
    methods: ["getHighEntropyValues", "toJSON"].map(k => d(k).value.name + "/" + d(k).value.length + " " + d(k).enumerable + " " + String(d(k).value).includes("[native code]")),
    illegal,
    ctor,
    json: JSON.parse(JSON.stringify(u)).platform,
  };
})()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    page.close();
    assert_eq!(
        actual,
        json!({
            "tag": "[object NavigatorUAData]",
            "proto": true,
            "own": [],
            "keys": ["brands", "mobile", "platform", "getHighEntropyValues", "toJSON", "constructor"],
            "getters": ["function true get brands true", "function true get mobile true", "function true get platform true"],
            "methods": ["getHighEntropyValues/1 true true", "toJSON/0 true true"],
            "illegal": "TypeError: Illegal invocation",
            "ctor": "TypeError: Failed to construct 'NavigatorUAData': Illegal constructor | Illegal constructor",
            "json": "macOS",
        })
    );
}
