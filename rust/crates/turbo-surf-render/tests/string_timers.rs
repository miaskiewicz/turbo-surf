//! String-handler window timers (`setTimeout("code")`): compiled lazily in the window's
//! global scope and admitted by the document's CSP `script-src` like eval. Expected values
//! are Chrome 156 (156.0.8078.4, headless, localhost page with the same response headers).
use serde_json::{json, Value};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

const COLLECT: &str = "<!doctype html><body><script nonce=\"n\">
globalThis.events=[];document.addEventListener('securitypolicyviolation',e=>events.push({d:e.violatedDirective,e:e.effectiveDirective,b:e.blockedURI,s:e.sample,disp:e.disposition}));
globalThis.ran=[];var probeVar=1;
</script></body>";

async fn open(headers: &[(&str, &str)]) -> PageSession {
    let headers = headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect::<Vec<_>>();
    PageSession::open_with_response_headers(
        COLLECT,
        "http://timers.test/page",
        "",
        "",
        5000,
        "",
        ClientHints::default(),
        &headers,
    )
    .await
    .unwrap()
}

async fn json(page: &mut PageSession, expression: &str) -> Value {
    serde_json::from_str(&page.eval_json_expression(expression).await.unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn string_timers_run_in_the_global_scope_when_eval_is_allowed() {
    let mut page = open(&[]).await;
    page.eval(
        "globalThis.ids=[setTimeout(\"ran.push(typeof probeVar, this===globalThis, document.currentScript)\",0),setInterval('ran.push(\"tick\");if(ran.length>=3)clearInterval(ids[1])',0)];",
    )
    .await
    .unwrap();
    let value = json(
        &mut page,
        "({ran, events, positive: ids.every(id=>id>0), distinct: ids[0]!==ids[1]})",
    )
    .await;
    assert_eq!(
        value,
        json!({"ran":["number",true,null,"tick"],"events":[],"positive":true,"distinct":true})
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#63: fails until the fix"]
async fn enforced_script_src_blocks_string_timers_with_an_eval_violation() {
    let mut page = open(&[("Content-Security-Policy", "script-src 'nonce-n'")]).await;
    page.eval(
        "globalThis.ids=[setTimeout(\"  ran.push('blocked')\",0),setInterval('ran.push(\"interval\")',0)];",
    )
    .await
    .unwrap();
    let value = json(&mut page, "({ran, events, ids})").await;
    let violation = json!({"d":"script-src","e":"script-src","b":"eval","s":"","disp":"enforce"});
    assert_eq!(value["ran"], json!([]));
    assert_eq!(value["events"], json!([violation, violation]));
    // Chrome returns timer id 0 for a handler its CSP check rejects.
    assert_eq!(value["ids"], json!([0, 0]));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#63: fails until the fix"]
async fn report_only_script_src_reports_and_still_runs_string_timers() {
    let mut page = open(&[(
        "Content-Security-Policy-Report-Only",
        "script-src 'self' 'unsafe-inline' 'report-sample'",
    )])
    .await;
    page.eval("setTimeout(\"\\n ran.push('report-only timer with a sample longer than forty units')\",0);")
        .await
        .unwrap();
    let value = json(&mut page, "({ran, events})").await;
    assert_eq!(
        value,
        json!({
            "ran":["report-only timer with a sample longer than forty units"],
            "events":[{"d":"script-src","e":"script-src","b":"eval","s":"ran.push('report-only timer with a sam","disp":"report"}]
        })
    );
}
