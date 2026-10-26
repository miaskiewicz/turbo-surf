//! Web Animations on the real render runtime: the binding's live-timeline probe Chrome 156 ran
//! on its own frame clock, replayed against turbo-surf's rendering-frame scheduler.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;

const PROBE: &str = test_support::ANIMATIONS_FRAMES_PROBE_JS;
const CASES: &[u8] = test_support::ANIMATIONS_FRAMES_CASES_JSON;

async fn observe(html: &str, expression: &str) -> Value {
    let mut page = PageSession::open(html, "http://127.0.0.1/", "", "", 20000)
        .await
        .unwrap();
    let raw = page
        .eval_json_expression(expression.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    serde_json::from_str(&raw).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn animations_follow_the_frame_clock_like_chrome156() {
    let control: Value =
        serde_json::from_str(test_support::ANIMATIONS_FRAMES_CHROME156_JSON).unwrap();
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE.as_bytes()))
    );
    assert_eq!(
        control["casesSha256"],
        format!("{:x}", Sha256::digest(CASES))
    );
    let cases: Value = serde_json::from_slice(CASES).unwrap();
    let actual = observe(cases[0]["html"].as_str().unwrap(), PROBE).await;
    let expected = &control["result"][0]["result"];
    let rows: Vec<String> = expected
        .as_array()
        .unwrap()
        .iter()
        .zip(actual.as_array().unwrap())
        .filter(|(chrome, native)| chrome != native)
        .map(|(chrome, native)| format!("native={native}\n chrome={chrome}"))
        .collect();
    assert!(rows.is_empty(), "{}", rows.join("\n"));
    assert_eq!(&actual, expected);
}

// The frame host is installed once by trusted bootstrap; page script cannot replace it, and
// timers or requestAnimationFrame replaced by the page do not stall animation frames.
#[tokio::test(flavor = "current_thread")]
async fn animation_frames_stay_engine_owned() {
    let actual = observe(
        "<!doctype html><body><div id=t></div>",
        r#"(async () => {
          const ns = globalThis[Symbol.for('ts')];
          const host = typeof ns.installAnimationHost;
          const t = document.getElementById('t');
          globalThis.setTimeout = () => { throw new Error('guest timer'); };
          globalThis.requestAnimationFrame = () => { throw new Error('guest frame'); };
          const a = t.animate({ opacity: [0, 1] }, 60);
          const events = [];
          a.onfinish = (event) => events.push([event.type, event.isTrusted, event.currentTime]);
          await a.finished;
          await new Promise((resolve) => { a.addEventListener('finish', () => {}); queueMicrotask(resolve); });
          return [host, a.playState, getComputedStyle(t).opacity, document.getAnimations().length, events.length <= 1];
        })()"#,
    )
    .await;
    assert_eq!(
        actual,
        serde_json::json!(["undefined", "finished", "1", 0, true])
    );
}
