//! DOMParser, main-document declarative shadow scripts and the Trusted Types
//! sinks of the HTML Sanitizer entry points against the Chrome 156 control
//! captured by harness/detection/sanitizer-chrome.mjs.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

const CASES: &str = include_str!("../../../../harness/detection/sanitizer-cases.json");
const FIXTURE: &str =
    include_str!("../../../../harness/detection/fixtures/sanitizer-chrome156.json");

fn harness_file(name: &str) -> &'static str {
    match name {
        "sanitizer-domparser-probe.js" => {
            include_str!("../../../../harness/detection/sanitizer-domparser-probe.js")
        }
        "sanitizer-declarative-scripts-probe.js" => {
            include_str!("../../../../harness/detection/sanitizer-declarative-scripts-probe.js")
        }
        "sanitizer-declarative-scripts.html" => {
            include_str!("../../../../harness/detection/sanitizer-declarative-scripts.html")
        }
        "sanitizer-trusted-types-probe.js" => {
            include_str!("../../../../harness/detection/sanitizer-trusted-types-probe.js")
        }
        other => panic!("unknown harness file {other}"),
    }
}

fn case(name: &str) -> Value {
    let cases: Value = serde_json::from_str(CASES).unwrap();
    cases
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap()
        .clone()
}

// The control is only valid for the exact probe and page bytes Chrome executed.
fn oracle(name: &str) -> Value {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture["browser"], "Chrome/156.0.8078.47");
    let entry = case(name);
    for key in ["probe", "page"] {
        if let Some(file) = entry[key].as_str() {
            assert_eq!(
                fixture["fixtures"][file],
                format!("{:x}", Sha256::digest(harness_file(file))),
                "{file}"
            );
        }
    }
    fixture["result"][name].clone()
}

async fn observe(name: &str) -> Value {
    let entry = case(name);
    let page = entry["page"].as_str().map_or(
        "<!doctype html><html><head></head><body></body></html>",
        harness_file,
    );
    let headers = entry["headers"]
        .as_object()
        .map_or_else(Vec::new, |headers| {
            headers
                .iter()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_string()))
                .collect::<Vec<_>>()
        });
    let mut session = PageSession::open_with_response_headers(
        page,
        &format!("http://127.0.0.1/{name}"),
        "",
        "",
        20000,
        "",
        ClientHints::default(),
        &headers,
    )
    .await
    .unwrap();
    let probe = harness_file(entry["probe"].as_str().unwrap());
    let raw = session
        .eval_json_expression(probe.trim().trim_end_matches(';'))
        .await
        .unwrap();
    session.close();
    serde_json::from_str(&raw).unwrap()
}

fn differences(actual: &Value, expected: &Value) -> Vec<String> {
    let mut rows = Vec::new();
    for (section, cases) in expected.as_object().unwrap() {
        match cases.as_object() {
            Some(cases) => {
                for (key, value) in cases {
                    if actual[section].get(key) != Some(value) {
                        rows.push(format!(
                            "{section}.{key}: native={} Chrome={value}",
                            actual[section][key]
                        ));
                    }
                }
            }
            None if &actual[section] != cases => rows.push(format!(
                "{section}: native={} Chrome={cases}",
                actual[section]
            )),
            None => {}
        }
    }
    rows
}

#[tokio::test(flavor = "current_thread")]
async fn dom_parser_matches_chrome156_except_xml_documents() {
    let mut actual = observe("domparser").await;
    let mut expected = oracle("domparser");
    // KNOWN_GAP: the HTML Tree engine has no XML parser, so the four XML
    // SupportedType values reject instead of returning an XMLDocument.
    let gap = serde_json::json!({
        "error": "NotSupportedError",
        "message": "XML document parsing is unsupported by the HTML Tree engine"
    });
    for key in ["applicationXml", "svg", "xhtml", "textXml"] {
        assert_eq!(actual["xml"][key], gap, "{key}");
        assert_ne!(expected["xml"][key], gap, "{key}");
    }
    actual.as_object_mut().unwrap().remove("xml");
    expected.as_object_mut().unwrap().remove("xml");
    let rows = differences(&actual, &expected);
    assert!(rows.is_empty(), "{}", rows.join("\n"));
    assert_eq!(actual, expected);
}

#[tokio::test(flavor = "current_thread")]
async fn declarative_shadow_scripts_run_in_parse_order_like_chrome156() {
    let actual = observe("declarative-scripts").await;
    let expected = oracle("declarative-scripts");
    let rows = differences(&actual, &expected);
    assert!(rows.is_empty(), "{}", rows.join("\n"));
    assert_eq!(actual, expected);
}

#[tokio::test(flavor = "current_thread")]
async fn sanitizer_entry_points_enforce_trusted_types_like_chrome156() {
    let actual = observe("trusted-types").await;
    let expected = oracle("trusted-types");
    let rows = differences(&actual, &expected);
    assert!(rows.is_empty(), "{}", rows.join("\n"));
    assert_eq!(actual, expected);
}
