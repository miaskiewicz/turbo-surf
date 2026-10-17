use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const CASES: &str = include_str!("../../../../harness/detection/trusted-types-cases.json");
const PROBE: &str = include_str!("../../../../harness/detection/trusted-types-probe.js");
const CONTROL: &str = include_str!("../../../../harness/detection/trusted-types-chrome155.json");
const SINK_CASES: &str =
    include_str!("../../../../harness/detection/trusted-types-sinks-cases.json");
const SINK_PROBE: &str = include_str!("../../../../harness/detection/trusted-types-sinks-probe.js");
const SINK_CONTROL: &str =
    include_str!("../../../../harness/detection/trusted-types-sinks-chrome155.json");

async fn compare(cases: &str, probe: &str, control: &str) {
    compare_collector(
        cases,
        probe,
        control,
        include_bytes!("../../../../harness/detection/trusted-types-chrome.mjs"),
    )
    .await;
}

async fn compare_collector(cases: &str, probe: &str, control: &str, collector: &[u8]) {
    let oracle: Value = serde_json::from_str(control).unwrap();
    for (name, bytes) in [
        ("casesSha256", cases.as_bytes()),
        ("probeSha256", probe.as_bytes()),
        ("collectorSha256", collector),
    ] {
        assert_eq!(
            oracle[name],
            format!("{:x}", Sha256::digest(bytes)),
            "stale {name}"
        );
    }
    assert_eq!(oracle["errors"], serde_json::json!([]));
    assert!(matches!(
        oracle["browser"].as_str(),
        Some("Chrome/155.0.8059.79" | "Chrome/155.0.8059.80")
    ));
    assert_eq!(oracle["v8"], "15.5.35.23");
    let cases: Value = serde_json::from_str(cases).unwrap();
    for case in cases.as_array().unwrap() {
        let expected = oracle["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == case["id"])
            .unwrap();
        let headers = case["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row[0].as_str().unwrap().to_string(),
                    row[1].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>();
        let mut page = PageSession::open_with_response_headers(
            case["html"].as_str().unwrap(),
            "https://trusted.test/control",
            "",
            "",
            15000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let actual = page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", case["id"]));
        // Compare JSON inside V8 so valid lone UTF-16 units are retained.
        let comparison = format!(
            "((a,b)=>{{const canonical=v=>JSON.stringify(v,(_,x)=>x&&typeof x==='object'&&!Array.isArray(x)?Object.fromEntries(Object.keys(x).sort().map(k=>[k,x[k]])):x);return canonical(a)===canonical(b)}})(JSON.parse({actual:?}),JSON.parse({:?}))",
            expected["result"].to_string()
        );
        let equal = page.eval_json_expression(&comparison).await.unwrap();
        assert_eq!(
            equal, "true",
            "{} actual={actual}\nexpected={}",
            case["id"], expected["result"]
        );
        page.close();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_trusted_types_values_policies_and_security_reports_match_chrome() {
    compare(CASES, PROBE, CONTROL).await;
}

#[tokio::test(flavor = "current_thread")]
async fn native_trusted_types_dom_sinks_default_policy_and_reports_match_chrome() {
    compare(SINK_CASES, SINK_PROBE, SINK_CONTROL).await;
}

#[tokio::test(flavor = "current_thread")]
async fn native_trusted_script_eval_and_timer_sources_match_chrome() {
    if !cfg!(turbo_surf_native_proxy) {
        return;
    }
    compare_collector(
        include_str!("../../../../harness/detection/trusted-types-code-cases.json"),
        include_str!("../../../../harness/detection/trusted-types-code-probe.js"),
        include_str!("../../../../harness/detection/trusted-types-code-chrome155.json"),
        include_bytes!("../../../../harness/detection/trusted-types-code-chrome.mjs"),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn published_sdk_fails_closed_for_enforced_raw_eval_without_native_callback() {
    if cfg!(turbo_surf_native_proxy) {
        return;
    }
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body></body>",
        "https://trusted.test/control",
        "",
        "",
        15000,
        "",
        Default::default(),
        &[(
            "Content-Security-Policy".into(),
            "require-trusted-types-for 'script'".into(),
        )],
    )
    .await
    .unwrap();
    let result=page.eval_json_expression("(()=>{try{eval('globalThis.rawScriptExecuted=true')}catch(error){return error.name==='EvalError'&&globalThis.rawScriptExecuted===undefined}return false})()").await.unwrap();
    assert_eq!(result, "true");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn native_trusted_types_callbacks_release_retired_frames_unless_guest_retained() {
    const CASES: &str =
        include_str!("../../../../harness/detection/trusted-types-lifecycle-cases.json");
    const PROBE: &str =
        include_str!("../../../../harness/detection/trusted-types-lifecycle-probe.js");
    const CONTROL: &str =
        include_str!("../../../../harness/detection/trusted-types-lifecycle-chrome155.json");
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    for (name, bytes) in [
        ("casesSha256", CASES.as_bytes()),
        ("probeSha256", PROBE.as_bytes()),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/trusted-types-lifecycle-chrome.mjs")
                .as_slice(),
        ),
    ] {
        assert_eq!(
            control[name],
            format!("{:x}", Sha256::digest(bytes)),
            "stale {name}"
        );
    }
    assert_eq!(control["errors"], serde_json::json!([]));
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let case = &cases[0];
    let headers = case["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row[0].as_str().unwrap().to_string(),
                row[1].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let mut page = PageSession::open_with_response_headers(
        case["html"].as_str().unwrap(),
        "https://trusted.test/control",
        "",
        "",
        15000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    assert_eq!(
        page.eval_json_expression(PROBE.trim().trim_end_matches(';'))
            .await
            .unwrap(),
        "\"prepared\""
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let first: Value = serde_json::from_str(
        &page
            .eval_json_expression("__readTrustedTypesLifetime()")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first, control["result"][0]["result"]["first"]);
    page.eval_json_expression("globalThis.__trustedTypesLifetime.retained=null; 'released'")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let released: Value = serde_json::from_str(
        &page
            .eval_json_expression("__readTrustedTypesLifetime()")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(released, control["result"][0]["result"]["afterRelease"]);
    page.close();
}
