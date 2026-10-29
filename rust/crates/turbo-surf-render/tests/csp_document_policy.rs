//! Authored normal Chrome controls for frozen base URLs and live meta policies.
use serde_json::Value;
use turbo_surf_render::PageSession;

async fn run(probe: &str, control: &str) {
    // A generous cap for real work, not a timing (turbo-surf#46).
    let mut page = PageSession::open(
        "<!doctype html><head></head><body></body>",
        "http://localhost/",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let probe = probe.trim().trim_end_matches(';');
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let expected: Value = serde_json::from_str(control).unwrap();
    assert_eq!(actual, expected["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn native_frozen_base_urls_and_policy_timing_match_chrome() {
    run(
        include_str!("../../../../harness/detection/csp-base-probe.js"),
        include_str!("../../../../harness/detection/fixtures/csp-base-chrome155.json"),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn native_meta_policy_mutations_intersect_and_follow_head_membership() {
    run(
        include_str!("../../../../harness/detection/csp-meta-probe.js"),
        include_str!("../../../../harness/detection/fixtures/csp-meta-chrome155.json"),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn native_base_policy_ignores_guest_tree_hooks_and_prototype_replacements() {
    let mut page = PageSession::open(
        "<!doctype html><head></head><body></body>",
        "https://source.test/root",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let raw=page.eval_json_expression("(()=>{const base=document.createElement('base');base.href='/allowed/';document.head.appendChild(base);let reads=0;Document.prototype.querySelectorAll=()=>{reads++;throw Error('guest query')};Element.prototype.getAttribute=()=>{reads++;throw Error('guest attr')};Object.defineProperty(base,'ownerDocument',{get(){reads++;throw Error('guest owner')}});const element=document.createElement('div');document.body.appendChild(element);return {document:document.baseURI,node:element.baseURI,reads};})()").await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        actual,
        serde_json::json!({"document":"https://source.test/allowed/","node":"https://source.test/allowed/","reads":0})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_base_parser_and_fragment_ordering_preserve_frozen_urls_and_namespace() {
    run(
        include_str!("../../../../harness/detection/csp-base-order-probe.js"),
        include_str!("../../../../harness/detection/fixtures/csp-base-order-chrome155.json"),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn retained_retired_documents_reset_written_bases_to_fallback_like_chrome() {
    run(
        include_str!("../../../../harness/detection/csp-base-retired-probe.js"),
        include_str!("../../../../harness/detection/fixtures/csp-base-retired-chrome155.json"),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn opaque_srcdoc_bootstrap_and_live_base_ignore_public_getter_poisoning() {
    run(
        include_str!("../../../../harness/detection/csp-base-opaque-probe.js"),
        include_str!("../../../../harness/detection/fixtures/csp-base-opaque-chrome156.json"),
    )
    .await;
}
