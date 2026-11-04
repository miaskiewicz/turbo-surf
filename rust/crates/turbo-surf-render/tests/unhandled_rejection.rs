//! turbo-surf#103: unhandledrejection / rejectionhandled on Window with Chrome 156's
//! timing (a task after the rejecting task's microtasks), PromiseRejectionEvent shape,
//! preventDefault, the event handler attribute and per-realm dispatch
//! (tests/fixtures/unhandled_rejection_*; collector tests/fixture-gen/i103-i105-chrome.mjs).
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("fixtures/unhandled_rejection_probe.js");
const CONTROL: &str = include_str!("fixtures/unhandled_rejection_chrome156.json");

#[tokio::test(flavor = "current_thread")]
async fn promise_rejection_events_match_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "http://127.0.0.1/",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(PROBE.trim_end().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let expected = control["result"].as_array().unwrap();
    let actual = actual.as_array().unwrap();
    for (index, (native, chrome)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(native, chrome, "entry {index}");
    }
    assert_eq!(actual.len(), expected.len(), "native log: {actual:?}");
}

/// A rejection no realm takes (a frame realm before or after its tracker, or once
/// removed) is logged, never fatal: the session keeps running tasks and evaluating.
#[tokio::test(flavor = "current_thread")]
async fn unrouted_rejections_never_abort_the_session() {
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "http://127.0.0.1/",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(
            r#"(async()=>{
              const f=document.createElement("iframe");document.body.append(f);
              const w=f.contentWindow;
              w.Promise.reject(new w.Error("early"));
              f.remove();
              for(let i=0;i<20;i++)w.Promise.reject(new w.Error("removed"));
              await new Promise(r=>setTimeout(r,30));
              return "alive";
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(raw, "\"alive\"");
    let again = page.eval_json_expression("1+1").await.unwrap();
    assert_eq!(again, "2");
    page.close();
}
