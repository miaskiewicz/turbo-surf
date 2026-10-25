//! Element/Window scrolling and checkVisibility against the authored Chrome156
//! control (fixed-pixel scrollers, 1280x800 viewport, device scale 2).
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const SCROLL: &str = include_str!("../../../../harness/detection/element-scroll-probe.js");
const EVENTS: &str = include_str!("../../../../harness/detection/element-scroll-events-probe.js");
const VISIBILITY: &str = include_str!("../../../../harness/detection/element-visibility-probe.js");

// The saved control is only valid for the exact probe bytes Chrome executed.
fn oracle(probe: &str, source: &str) -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/element-scroll-chrome156.json"
    ))
    .unwrap();
    assert_eq!(fixture["browser"], "Chrome/156.0.8078.47");
    assert_eq!(
        fixture["fixtures"][probe],
        format!("{:x}", Sha256::digest(source))
    );
    fixture["result"][probe].clone()
}

async fn observe(probe: &str) -> Value {
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "http://127.0.0.1/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(probe.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    serde_json::from_str(&raw).unwrap()
}

fn mismatches(actual: &Value, expected: &Value) -> Vec<String> {
    let (Some(actual), Some(expected)) = (actual.as_object(), expected.as_object()) else {
        return vec![];
    };
    expected
        .iter()
        .filter(|(key, value)| actual.get(*key) != Some(*value))
        .map(|(key, value)| format!("{key}: native={} Chrome={value}", actual[key]))
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn scroll_methods_window_offsets_and_scroll_into_view_match_chrome156() {
    let actual = observe(SCROLL).await;
    let expected = oracle("element-scroll-probe.js", SCROLL);
    let differences = mismatches(&actual, &expected);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
    assert_eq!(actual, expected);
}

// Chrome interleaves its own frame production with the posted resolution and
// zero-delay tasks, so those marks are compared only for presence. The scroll
// and scrollend rows, their order and their placement after the synchronous turn
// and before the frame's animation callbacks are exact.
fn frame_shape(rows: &Value) -> (Vec<Value>, bool, Vec<Value>) {
    let rows = rows.as_array().unwrap();
    let events: Vec<Value> = rows
        .iter()
        .filter(|row| row.as_array().unwrap().len() > 1 && row[0] != "state")
        .cloned()
        .collect();
    let resolved = rows.iter().any(|row| row[0] == "resolved");
    let marks: Vec<Value> = rows
        .iter()
        .filter(|row| {
            let label = &row[0];
            row.as_array().unwrap().len() == 1 && label != "resolved" && label != "timeout0"
        })
        .cloned()
        .collect();
    (events, resolved, marks)
}

#[tokio::test(flavor = "current_thread")]
async fn scroll_events_coalesce_per_box_before_animation_callbacks_like_chrome156() {
    let actual = observe(EVENTS).await;
    let expected = oracle("element-scroll-events-probe.js", EVENTS);
    let actual = actual["events"].as_array().unwrap();
    let expected = expected["events"].as_array().unwrap();
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual[0], expected[0]);
        assert_eq!(
            frame_shape(&actual[1]),
            frame_shape(&expected[1]),
            "{}",
            actual[0]
        );
        // Within the delivering frame, events precede every animation callback.
        let rows = actual[1].as_array().unwrap();
        let first_frame = rows.iter().position(|row| row[0] == "rafBefore").unwrap();
        assert!(rows[first_frame..]
            .iter()
            .all(|row| row.as_array().unwrap().len() == 1 || row[0] == "state"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn check_visibility_uses_real_boxes_and_flat_tree_styles_like_chrome156() {
    let actual = observe(VISIBILITY).await;
    let expected = oracle("element-visibility-probe.js", VISIBILITY);
    let differences = mismatches(&actual, &expected);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[tokio::test(flavor = "current_thread")]
async fn scroll_capabilities_stay_private_and_survive_public_intrinsic_poisoning() {
    let mut page = PageSession::open(
        "<!doctype html><body style='margin:0'><div id=box style='width:50px;height:50px;overflow:auto'><div style='height:500px'></div></div><div style='height:3000px'></div></body>",
        "http://127.0.0.1/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(
            r#"(async () => {
      const box = document.getElementById('box'), seen = [];
      box.addEventListener('scroll', (event) => { seen[seen.length] = [event.type, event.isTrusted, box.scrollTop]; });
      Array.prototype.includes = Array.prototype.push = Array.prototype.splice = Array.prototype.indexOf = () => { throw Error('public array'); };
      Array.prototype[Symbol.iterator] = () => { throw Error('public iterator'); };
      globalThis.Event = function () { throw Error('public Event'); };
      globalThis.setTimeout = () => { throw Error('public timer'); };
      Promise.prototype.then = Promise.prototype.then;
      const result = await box.scroll(0, 30);
      const forged = Object.create(Element.prototype);
      const rejected = await Element.prototype.scrollBy.call(forged, 0, 1).then(() => 'resolved', (error) => error.message);
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
      const smooth = box.scroll({ top: 10, behavior: 'smooth' });
      const before = box.scrollTop;
      const interrupted = box.scroll({ top: 20, behavior: 'smooth' });
      const outcomes = [await smooth, await interrupted];
      window.scrollY = 'replaced';
      return { result, rejected, seen, before, outcomes, after: box.scrollTop, replaced: Object.getOwnPropertyDescriptor(window, 'scrollY'),
        privateOps: ['op_live_element_scroll_to', 'op_capture_frame_scheduler'].some((name) => name in globalThis) };
    })()"#,
        )
        .await
        .unwrap();
    page.close();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        actual,
        serde_json::json!({
            "result": {"interrupted": false},
            "rejected": "Failed to execute 'scrollBy' on 'Element': Illegal invocation",
            "seen": [["scroll", true, 30], ["scroll", true, 20]],
            "before": 30,
            "outcomes": [{"interrupted": true}, {"interrupted": false}],
            "after": 20,
            "replaced": {"value": "replaced", "writable": true, "enumerable": true, "configurable": true},
            "privateOps": false
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn frame_windows_install_their_own_viewport_scroll_members() {
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "http://127.0.0.1/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(
            r#"(async () => {
      const frame = document.createElement('iframe');
      frame.srcdoc = '<body style="margin:0"><div style="height:900px"></div><script>scrollTo(0, 40).then((result) => parent.postMessage([scrollY, document.documentElement.scrollTop, result.interrupted, String(window.scroll)], "*"))<\/script>';
      const message = new Promise((done) => addEventListener('message', (event) => done(event.data)));
      document.body.append(frame);
      return [await message, scrollY];
    })()"#,
        )
        .await
        .unwrap();
    page.close();
    // The default 300x150 frame viewport scrolls its own 900px document.
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        serde_json::json!([[40, 40, false, "function scroll() { [native code] }"], 0])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn window_offsets_never_throw_when_native_layout_rejects_the_document() {
    let mut page = PageSession::open(
        "<!doctype html><body style='margin:0'><div style='height:3000px'></div></body>",
        "http://127.0.0.1/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(
            r#"(async () => {
      await scrollTo(0, 70);
      document.head.append(document.createElementNS('urn:fixture', 'meta'));
      const rejected = await scrollTo(0, 90).then(() => null, (error) => error.message);
      return [scrollY, pageYOffset, rejected];
    })()"#,
        )
        .await
        .unwrap();
    page.close();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(actual[0], 70);
    assert_eq!(actual[1], 70);
    assert_eq!(
        actual[2],
        "native live layout does not yet support this element namespace"
    );
}
