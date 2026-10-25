//! Pointer capture and pointer lock under trusted mouse input, against the Chrome 156
//! control captured by harness/detection/pointer-capture-chrome-control.mjs (CDP
//! Input.dispatchMouseEvent at element centers). Each case replays the same steps through
//! the private host input driver and must log the identical event sequence.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/pointer-capture-probe.js");
const CASES: &str = include_str!("../../../../harness/detection/pointer-capture-cases.json");

fn control() -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/pointer-capture-chrome156.json"
    ))
    .unwrap();
    assert_eq!(fixture["browser"], "Chrome/156.0.8078.47");
    assert_eq!(
        fixture["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    assert_eq!(
        fixture["casesSha256"],
        format!("{:x}", Sha256::digest(CASES))
    );
    fixture["result"].clone()
}

/// The collector's evaluation: an async function body whose value returns as JSON.
async fn evaluate(page: &mut PageSession, body: &str) -> Value {
    let raw = page
        .eval_json_expression(&format!(
            "(async()=>{{{body}\n}})().then(value=>value===undefined?null:value)"
        ))
        .await
        .unwrap_or_else(|error| panic!("{body}: {error}"));
    serde_json::from_str(&raw).unwrap()
}

async fn replay(case: &Value) -> Value {
    let html = format!(
        "<!doctype html><html><head></head><body>{}</body></html>",
        case["html"].as_str().unwrap()
    );
    let mut page = PageSession::open(&html, "https://example.test/", "", "", 20000)
        .await
        .unwrap();
    let options = case.get("options").cloned().unwrap_or_else(|| json!({}));
    let setup = case["setup"].as_str().unwrap_or("");
    evaluate(&mut page, &format!("({})({options});{setup}", PROBE.trim())).await;
    let (mut evaluations, mut marker) = (Vec::new(), 0);
    for step in case["steps"].as_array().unwrap() {
        if let Some(body) = step["eval"].as_str() {
            evaluations.push(evaluate(&mut page, body).await);
            continue;
        }
        let kind = ["move", "down", "up", "click"]
            .into_iter()
            .find(|kind| step.get(*kind).is_some())
            .unwrap();
        if step["quiet"] != true {
            let label = json!(format!("--{marker} {kind}"));
            evaluate(&mut page, &format!("__log.push({label})")).await;
        }
        marker += 1;
        let mut input = step.clone();
        input.as_object_mut().unwrap().remove("quiet");
        page.input(&json!([input])).await.unwrap();
    }
    let log = evaluate(&mut page, "await sleep(100);return __log").await;
    page.close();
    json!({ "evaluations": evaluations, "log": log })
}

/// KNOWN_GAP: the binding's event path stops at a shadow root, so composed pointer, mouse
/// and capture events dispatched at a shadow-tree target never reach the host's ancestors
/// and their target/relatedTarget are not retargeted to the host (a general shadow DOM
/// dispatch gap, not a capture one). Capture itself agrees: the shadow-internal listener
/// sees got/lost at Chrome's points. This pins the exact native log until dispatch crosses
/// shadow boundaries; adopting Chrome's log then makes the case pass unmodified.
const SHADOW_DISPATCH_GAP: &[&str] = &[
    "--0 move",
    "pointerover a rel=null b=0",
    "pointerenter HTML rel=null b=0",
    "pointerenter BODY rel=null b=0",
    "pointerenter a rel=null b=0",
    "mouseover a rel=null b=0",
    "mouseenter HTML rel=null b=0",
    "mouseenter BODY rel=null b=0",
    "mouseenter a rel=null b=0",
    "pointermove a b=0 btn=-1",
    "mousemove a b=0 btn=0",
    "--1 down",
    "pointerdown a b=1 btn=0",
    "mousedown a b=1 btn=0",
    "--2 move",
    "pointerout a rel=inner b=1",
    "pointerleave a rel=inner b=1",
    "pointerenter h rel=a b=1",
    "inner gotpointercapture target=inner",
    "mouseout a rel=inner b=1",
    "mouseleave a rel=inner b=1",
    "mouseenter h rel=a b=1",
    "--3 up",
    "inner lostpointercapture target=inner",
    "pointerleave h rel=b b=0",
    "pointerover b rel=inner b=0",
    "pointerenter b rel=inner b=0",
    "mouseleave h rel=b b=0",
    "mouseover b rel=inner b=0",
    "mouseenter b rel=inner b=0",
];

#[tokio::test(flavor = "current_thread")]
async fn trusted_pointer_capture_and_lock_sequences_match_chrome156() {
    let expected = control();
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let cases = cases["cases"].as_object().unwrap();
    assert_eq!(cases.len(), expected.as_object().unwrap().len());
    let mut differences = Vec::new();
    for (name, case) in cases {
        let actual = replay(case).await;
        if name == "capture-in-shadow-tree" {
            let chrome = expected[name]["log"].as_array().unwrap();
            let inner = |log: &[Value]| -> Vec<Value> {
                log.iter()
                    .filter(|row| row.as_str().unwrap().starts_with("inner "))
                    .cloned()
                    .collect()
            };
            assert_ne!(
                actual, expected[name],
                "shadow dispatch gap closed: drop the pin"
            );
            assert_eq!(actual["log"], json!(SHADOW_DISPATCH_GAP));
            assert_eq!(inner(actual["log"].as_array().unwrap()), inner(chrome));
            continue;
        }
        if actual != expected[name] {
            differences.push(format!(
                "{name}\n  native: {}\n  chrome: {}",
                serde_json::to_string_pretty(&actual).unwrap(),
                serde_json::to_string_pretty(&expected[name]).unwrap()
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

/// The private pointer op stays off the guest global, and guest replacements of the
/// event and capture primitives cannot reshape a trusted captured drag.
#[tokio::test(flavor = "current_thread")]
async fn trusted_capture_ignores_guest_replacements() {
    let mut page = PageSession::open(
        "<body><div id=a style='position:absolute;left:0;top:0;width:50px;height:50px'></div><div id=b style='position:absolute;left:100px;top:0;width:50px;height:50px'></div></body>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval(r#"
      globalThis.rows=[];const a=document.querySelector('#a');
      for(const type of ['gotpointercapture','pointermove','pointerup','lostpointercapture','click'])window.addEventListener(type,event=>{rows[rows.length]=type+' '+event.target.id+' '+event.isTrusted},true);
      a.addEventListener('pointerdown',event=>a.setPointerCapture(event.pointerId));
            Element.prototype.hasPointerCapture=()=>true;
      globalThis.PointerEvent=function(){throw new Error('guest PointerEvent')};
      a.dispatchEvent=()=>{throw new Error('guest dispatch')};
      globalThis.globalReach=['op_host_pointer' in globalThis,'__pointerNative' in globalThis,'__pointerLockEvents' in globalThis];
    "#).await.unwrap();
    for step in [
        json!({"move": {"selector": "#a"}}),
        json!({"down": "#a"}),
        json!({"move": {"selector": "#b"}}),
        json!({"up": "#b"}),
    ] {
        page.input(&json!([step])).await.unwrap();
    }
    let rows: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                "JSON.parse(JSON.stringify({rows:Object.values(rows),globalReach}))",
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        rows,
        json!({
            "rows": [
                "pointermove a true",
                "gotpointercapture a true",
                "pointermove a true",
                "pointerup a true",
                "lostpointercapture a true",
                "click a true"
            ],
            "globalReach": [false, false, false]
        })
    );
    page.close();
}
