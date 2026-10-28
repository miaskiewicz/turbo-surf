//! Topics and Private State Token Document members against the fresh-profile
//! Chrome 156 control: a loopback HTTP, an HTTPS and an insecure HTTP top origin.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/privacy-sandbox-probe.js");
const COLLECTOR: &[u8] = include_bytes!("../../../../harness/detection/privacy-sandbox-chrome.mjs");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/privacy_sandbox_chrome156.json");

/// The engine's document URL for each collected case (the collector's ports differ,
/// and no result depends on them).
fn base(id: &str) -> &'static str {
    match id {
        "loopback-http" => "http://127.0.0.1/",
        "localhost-https" => "https://localhost/",
        "insecure-http" => "http://insecure.test/",
        other => panic!("unknown case {other}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn privacy_sandbox_members_match_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    assert_eq!(
        control["collectorSha256"],
        format!("{:x}", Sha256::digest(COLLECTOR))
    );
    let cases = control["result"].as_array().unwrap();
    assert_eq!(cases.len(), 3);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let mut page = PageSession::open(
            "<!doctype html><html><head></head><body></body></html>",
            base(id),
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
        let expected = &case["result"];
        for (key, value) in expected.as_object().unwrap() {
            assert_eq!(&actual[key], value, "{id}: {key}");
        }
        assert_eq!(&actual, expected, "{id}");
    }
}

/// A top-frame origin keeps at most two issuer associations; repeating an associated
/// issuer still resolves, and redemption queries never associate.
#[tokio::test(flavor = "current_thread")]
async fn private_token_issuer_associations_follow_the_top_frame_origin() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://top.localhost/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(
            "(async()=>{const r=[];for(const i of ['https://a.example','https://b.example','https://c.example','https://a.example'])try{r.push(await document.hasPrivateToken(i))}catch(e){r.push(e.name)}r.push(await document.hasRedemptionRecord('https://c.example'));return r})()",
        )
        .await
        .unwrap();
    page.close();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        serde_json::json!([false, false, "OperationError", false, false])
    );
}

/// A file: top-level document is a secure context without an HTTP(S) top-frame origin:
/// both queries reject with Chrome 156's NotAllowedError (frozen Chrome on a file: page).
#[tokio::test(flavor = "current_thread")]
async fn private_token_queries_need_an_http_family_top_frame_origin() {
    let mut page = PageSession::open("<!doctype html><body>", "file:///probe.html", "", "", 20000)
        .await
        .unwrap();
    let raw = page
        .eval_json_expression(
            "(async()=>{const r=[isSecureContext];for(const m of ['hasPrivateToken','hasRedemptionRecord'])try{r.push(await document[m]('https://a.example'))}catch(e){r.push(e.name+': '+e.message)}return r})()",
        )
        .await
        .unwrap();
    page.close();
    let message = |member: &str| {
        format!("NotAllowedError: Failed to execute '{member}' on 'Document': {member}: Cannot execute in documents without secure, HTTP(S), top-frame origins.")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        serde_json::json!([
            true,
            message("hasPrivateToken"),
            message("hasRedemptionRecord")
        ])
    );
}
