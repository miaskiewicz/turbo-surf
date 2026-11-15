use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const DEFAULT_PROBE: &str = include_str!("../../../../harness/detection/csp-native-tt-probe.js");
const KEYWORD_PROBE: &str =
    include_str!("../../../../harness/detection/csp-native-tt-keyword-probe.js");

fn control(text: &str, probe: &str) -> Value {
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["browser"], "156.0.8078.47");
    assert_eq!(value["authored"], true);
    assert_eq!(
        value["probeSha256"],
        format!("{:x}", Sha256::digest(probe.as_bytes()))
    );
    value
}

fn without_profile(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("userAgent");
    value
}

async fn observe(probe: &str, mode: &str, profile: &str, header: &str) -> Value {
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body></body>",
        &format!("https://trusted.test/?mode={mode}&profile={profile}"),
        "",
        "",
        10_000,
        "",
        Default::default(),
        &[("Content-Security-Policy".into(), header.into())],
    )
    .await
    .unwrap();
    page.eval(probe).await.unwrap();
    let error = page
        .eval_json_expression("globalThis.__TT_ERROR??null")
        .await
        .unwrap();
    assert_eq!(error, "null", "authored probe failed: {error}");
    serde_json::from_str(
        &page
            .eval_json_expression("globalThis.__TT_RESULT")
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn native_default_policy_labels_rewrites_and_throws_match_authored_chrome() {
    let expected = control(
        include_str!("../../../../harness/detection/fixtures/csp-native-tt-chrome156.json"),
        DEFAULT_PROBE,
    );
    assert_eq!(expected["rows"].as_array().unwrap().len(), 3);
    for (mode, row) in ["identity", "rewrite", "throw"]
        .iter()
        .zip(expected["rows"].as_array().unwrap())
    {
        assert!(row.get("error").is_none(), "invalid Chrome control {row}");
        let actual = observe(DEFAULT_PROBE, mode, "default", "script-src 'self' 'unsafe-eval'; require-trusted-types-for 'script'; trusted-types default probe").await;
        assert_eq!(
            without_profile(actual),
            without_profile(row["result"].clone()),
            "default policy mode {mode}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_keyword_no_default_policy_preserves_authored_chrome_admission_and_identity() {
    let expected = control(
        include_str!("../../../../harness/detection/fixtures/csp-native-tt-keyword-chrome156.json"),
        KEYWORD_PROBE,
    );
    assert_eq!(expected["rows"].as_array().unwrap().len(), 3);
    let profiles = [
        ("unsafe-eval-required", "script-src 'self' 'unsafe-eval'; require-trusted-types-for 'script'; trusted-types default probe"),
        ("trusted-eval-required", "script-src 'self' 'trusted-types-eval'; require-trusted-types-for 'script'; trusted-types default probe"),
        ("trusted-eval-optional", "script-src 'self' 'trusted-types-eval'; trusted-types default probe"),
    ];
    for ((name, header), row) in profiles.iter().zip(expected["rows"].as_array().unwrap()) {
        assert!(row.get("error").is_none(), "invalid Chrome control {row}");
        let actual = observe(KEYWORD_PROBE, "none", name, header).await;
        assert_eq!(
            without_profile(actual),
            without_profile(row["result"].clone()),
            "policy {name}"
        );
    }
}
