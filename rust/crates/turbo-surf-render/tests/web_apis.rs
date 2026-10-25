//! Observable, Subscriber, EventTarget#when and Blob#textStream against the
//! source-pinned Chrome 156 control, offline over an empty document.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const SHAPE: &str = include_str!("../../../../harness/detection/observable-shape-probe.js");
const SEMANTICS: &str = include_str!("../../../../harness/detection/observable-semantics-probe.js");
const TEXT_STREAM: &str = include_str!("../../../../harness/detection/blob-text-stream-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/web-apis-chrome156.json");

/// Measured Chrome behavior the engine does not reproduce, as `path = engine value`.
///
/// - `click()`: an untrusted event reaching a `when()` listener is attributed to
///   `dispatchEvent`; the vendored binding's `HTMLElement#click()` is not a frame
///   the Observable layer can see, so the inner context differs.
/// - The render tier dispatches no `unhandledrejection` events, so the promises
///   AsyncIteratorClose leaves rejected are never reported.
///
/// `xhr.when(...)` used to be a gap here (XMLHttpRequest was not an EventTarget),
/// but the merged real `XMLHttpRequestEventTarget -> EventTarget` now inherits
/// `when` and delivers events, matching Chrome 156 (which genuinely exposes
/// `EventTarget.prototype.when` in a fresh stable profile), so it is resolved.
const KNOWN_GAPS: &[&str] = &[
    "observable-semantics-probe.js/bindingContexts/log/2/1/1 = \"Failed to execute 'flatMap' on 'Observable': Failed to execute 'dispatchEvent' on 'EventTarget': Cannot convert value to an Observable. Input value must be an Observable, async iterable, iterable, or Promise.\"",
    "observable-semantics-probe.js/iteratorClose/log/7/1 = \"[]\"",
    "observable-semantics-probe.js/iteratorClose/log/9/1 = \"[]\"",
    "observable-semantics-probe.js/iteratorClose/log/11/1 = \"[]\"",
];

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    assert_eq!(control["v8"], "15.6.75.19");
    for (name, source) in [
        ("observable-shape-probe.js", SHAPE),
        ("observable-semantics-probe.js", SEMANTICS),
        ("blob-text-stream-probe.js", TEXT_STREAM),
    ] {
        assert_eq!(
            control["fixtures"][name],
            format!("{:x}", Sha256::digest(source)),
            "stale control: {name}"
        );
    }
    control
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
    let value: Value = serde_json::from_str(&raw).unwrap();
    // Probes return their record as a JSON string, as Chrome serializes it.
    serde_json::from_str(value.as_str().unwrap()).unwrap()
}

/// JSON-pointer paths where `actual` departs from Chrome, with the engine's value.
fn divergences(path: String, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Object(left), Value::Object(right)) => {
            let mut keys: Vec<_> = left.keys().chain(right.keys()).collect();
            keys.sort();
            keys.dedup();
            let missing = Value::String("<absent>".into());
            for key in keys {
                divergences(
                    format!("{path}/{key}"),
                    left.get(key).unwrap_or(&missing),
                    right.get(key).unwrap_or(&missing),
                    out,
                );
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                divergences(format!("{path}/{index}"), left, right, out);
            }
        }
        _ if actual != expected => out.push(format!("{path} = {actual}")),
        _ => {}
    }
}

#[tokio::test(flavor = "current_thread")]
async fn observable_when_and_text_stream_match_chrome156() {
    let control = control();
    let mut found = Vec::new();
    for (name, probe) in [
        ("observable-shape-probe.js", SHAPE),
        ("observable-semantics-probe.js", SEMANTICS),
        ("blob-text-stream-probe.js", TEXT_STREAM),
    ] {
        let actual = observe(probe).await;
        divergences(name.into(), &actual, &control["result"][name], &mut found);
    }
    assert_eq!(found, KNOWN_GAPS, "divergences from the Chrome control");
}

// Child realms install their own interfaces over the shared isolate records:
// a frame's Blob, Observable and EventTarget#when work there and across realms.
#[tokio::test(flavor = "current_thread")]
async fn child_realms_expose_working_observable_when_and_text_stream() {
    let probe = r#"(async () => {
      const frame = document.createElement("iframe");
      const loaded = new Promise((done) => (frame.onload = done));
      document.body.append(frame);
      await loaded;
      const child = frame.contentWindow;
      const read = async (stream) => (await stream.getReader().read()).value;
      const target = new child.EventTarget();
      const events = target.when("ping").map((event) => event.type).toArray();
      target.dispatchEvent(new child.Event("ping"));
      const values = await child.Observable.from([1, 2]).toArray();
      return JSON.stringify({
        distinct: child.Observable !== Observable,
        values: [values, Object.getPrototypeOf(values) === child.Array.prototype],
        text: await read(new child.Blob(["child"]).textStream()),
        borrowed: await read(Blob.prototype.textStream.call(new child.Blob(["borrowed"]))),
        foreign: await Observable.from(new child.Observable((s) => { s.next("x"); s.complete(); })).toArray(),
        pending: await Promise.race([events.then(() => "settled"), new Promise((done) => setTimeout(() => done("pending"), 0))]),
      });
    })()"#;
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "http://127.0.0.1/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page.eval_json_expression(probe).await.unwrap();
    page.close();
    let value: Value = serde_json::from_str(&raw).unwrap();
    let value: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "distinct": true,
            "values": [[1, 2], true],
            "text": "child",
            "borrowed": "borrowed",
            "foreign": ["x"],
            "pending": "pending",
        })
    );
}
