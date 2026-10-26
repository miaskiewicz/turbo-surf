//! File-state inputs, FileList and DataTransfer over the production File/Blob/FormData,
//! against the vendored binding's source-pinned Chrome 156 control.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;

const PROBE: &str = test_support::FILE_INPUTS_PROBE_JS;
const CASES: &str = test_support::FILE_INPUTS_CASES_JSON;
const CONTROL: &str = test_support::FILE_INPUTS_CHROME156_JSON;

/// Measured Chrome behavior the engine does not reproduce, as `path = engine value`
/// prefixes. These are the binding's own pinned gaps (Rusty V8 reports
/// ShouldThrowOnError as false; Blink's KURL escapes a space in a URL host).
const KNOWN_GAPS: &[&str] = &[
    ".strings[7][0] = \"http://c/\"",
    ".strictMutation[0][1] = \"Cannot assign to property '0' of #<FileList>\"",
    ".strictMutation[1][1] = \"Cannot assign to property '3' of #<FileList>\"",
    ".strictMutation[2][1] = \"Cannot delete property '0' of #<FileList>\"",
    ".strictMutation[4] = false",
    ".strictMutation[5] = false",
    ".strictMutation[6] = [\"TypeError\",\"Failed to set an indexed property [0] on 'FileList': Index property setter is not supported.\"]",
    ".itemListMutation[1][1] = \"Cannot assign to property '0' of #<DataTransferItemList>\"",
];

fn differences(path: &str, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                differences(&format!("{path}[{index}]"), a, b, out);
            }
        }
        (Value::Object(a), Value::Object(b)) if a.len() == b.len() => {
            for (key, b) in b {
                match a.get(key) {
                    Some(a) => differences(&format!("{path}.{key}"), a, b, out),
                    None => out.push(format!("{path}.{key}: missing")),
                }
            }
        }
        _ if actual != expected => out.push(format!("{path} = {actual} (Chrome {expected})")),
        _ => {}
    }
}

#[tokio::test(flavor = "current_thread")]
async fn file_inputs_match_chrome156_over_production_files() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    assert_eq!(
        control["casesSha256"],
        format!("{:x}", Sha256::digest(CASES))
    );
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let mut page = PageSession::open(
        cases[0]["html"].as_str().unwrap(),
        "http://127.0.0.1/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let mut out = Vec::new();
    differences("", &actual, &control["result"][0]["result"], &mut out);
    let unexpected: Vec<_> = out
        .iter()
        .filter(|difference| !KNOWN_GAPS.iter().any(|gap| difference.starts_with(gap)))
        .collect();
    let resolved: Vec<_> = KNOWN_GAPS
        .iter()
        .filter(|gap| !out.iter().any(|difference| difference.starts_with(**gap)))
        .collect();
    assert!(
        unexpected.is_empty() && resolved.is_empty(),
        "unexpected: {unexpected:#?}\nresolved gaps: {resolved:?}"
    );
}
