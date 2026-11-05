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
        .eval_engine_json_expression(expression.trim().trim_end_matches(';'))
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
          const ns = __tsNs;
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

/// turbo-dom#313: an infinite CSS animation (a loading spinner) keeps requesting rendering
/// frames, as in Chrome. A page that shows one still settles: opening it, an expression
/// evaluated on it and the async render all return well inside their budgets, after the
/// page's own load work ran, with the spinner still running.
#[tokio::test(flavor = "current_thread")]
async fn a_spinner_page_still_settles_within_the_budget() {
    const SPINNER: &str = "<!doctype html><html><head><style>@keyframes spin{to{transform:rotate(360deg)}}\
        .spinner{width:20px;height:20px;animation:spin 1s linear infinite}</style></head><body>\
        <div class=spinner id=s></div><script>addEventListener('load',()=>{document.title='loaded'})</script>\
        </body></html>";
    let started = std::time::Instant::now();
    let mut page = PageSession::open(SPINNER, "http://127.0.0.1/", "", "", 20_000)
        .await
        .unwrap();
    let state = page
        .eval_json_expression(
            "[document.title, document.getAnimations().length, document.getAnimations()[0].playState]",
        )
        .await
        .unwrap();
    page.close();
    assert_eq!(state, r#"["loaded",1,"running"]"#);
    let rendered = turbo_surf_render::render_html_async(
        SPINNER,
        "document.getElementById('s').dataset.ran = 'yes'",
    )
    .await
    .unwrap();
    assert!(rendered.contains(r#"data-ran="yes""#), "{rendered}");
    // A generous cap, not a timing: a frame loop the settle waited out would spend the
    // whole 20 s budget (and the render's 10 s) instead.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
}
