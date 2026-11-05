//! IndexedDB (open/upgradeneeded/versionchange, stores, indexes, cursors, key
//! ranges, structured clone, auto-commit, abort, errors, key order) against the
//! source-pinned Chrome 156 control,
//! offline over an empty document.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/indexeddb-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/indexeddb-chrome156.json");
const EVENTS_PROBE: &str = include_str!("../../../../harness/detection/indexeddb-events-probe.js");
const EVENTS_CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/indexeddb-events-chrome156.json");

fn control(name: &str, probe: &str, source: &str) -> Value {
    let control: Value = serde_json::from_str(source).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .ends_with("Chrome/156.0.8078.47"));
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(
        control["fixtures"][name],
        format!("{:x}", Sha256::digest(probe)),
        "stale control"
    );
    control["result"][name].clone()
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

/// Runs `probe` over an empty document and lists where it departs from `expected`.
async fn probe_divergences(probe: &str, expected: &Value) -> Vec<String> {
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
    let actual: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
    let mut found = Vec::new();
    divergences(String::new(), &actual, expected, &mut found);
    found
}

#[tokio::test(flavor = "current_thread")]
async fn indexeddb_matches_chrome156() {
    let expected = control("indexeddb-probe.js", PROBE, CONTROL);
    assert_eq!(
        probe_divergences(PROBE, &expected).await,
        Vec::<String>::new(),
        "divergences from the Chrome control"
    );
}

/// Measured Chrome behavior the engine does not reproduce yet, as `path = engine value`
/// (#148's remainder, pinned exactly so a fix or a new divergence fails here).
///
/// `/path`: Chrome builds one event path request -> transaction -> database, with capture
/// listeners on the database and transaction first (phase 1), and one event object. The
/// binding's dispatch has no "get the parent" for non-node targets, so surf fires a fresh
/// event at each target in turn and capture listeners there run in the bubbling phase.
/// `/microtasks`: Chrome checkpoints microtasks after every listener; the binding's
/// dispatcher has no per-listener host checkpoint (the same gap as #136), so surf
/// checkpoints after each target. `/deactivation`: Chrome deactivates a new transaction
/// at the end of its task; surf does so in a 0 ms timer, after timers queued earlier.
const EVENTS_KNOWN_GAPS: &[&str] = &[
    "/deactivation/1/1 = \"ok\"",
    "/microtasks/1 = \"s2\"",
    "/microtasks/2 = [\"s1-micro\",\"ok\"]",
    "/microtasks/5 = \"e2\"",
    "/microtasks/6 = [\"e1-micro\",\"ok\"]",
    "/path/full/events = 3",
    "/path/full/log/0/0 = \"req\"",
    "/path/full/log/0/1 = \"bubble\"",
    "/path/full/log/0/2 = 2",
    "/path/full/log/0/3 = \"req\"",
    "/path/full/log/0/6/0 = \"req\"",
    "/path/full/log/1/0 = \"req\"",
    "/path/full/log/1/2 = 2",
    "/path/full/log/1/3 = \"req\"",
    "/path/full/log/1/6/0 = \"req\"",
    "/path/full/log/2/0 = \"tx\"",
    "/path/full/log/2/2 = 3",
    "/path/full/log/2/3 = \"tx\"",
    "/path/full/log/2/5 = 1",
    "/path/full/log/2/6/0 = \"tx\"",
    "/path/full/log/3/0 = \"tx\"",
    "/path/full/log/3/2 = 3",
    "/path/full/log/3/3 = \"tx\"",
    "/path/full/log/3/5 = 1",
    "/path/full/log/3/6/0 = \"tx\"",
    "/path/full/log/4/0 = \"db\"",
    "/path/full/log/4/3 = \"db\"",
    "/path/full/log/4/5 = 2",
    "/path/full/log/4/6/0 = \"db\"",
    "/path/full/log/5/1 = \"capture\"",
    "/path/full/log/5/5 = 2",
    "/path/stopped/events = 3",
    "/path/stopped/log = [[\"req\",\"bubble\",2,\"req\",\"req\",0,[\"req\"]],[\"req\",\"capture\",2,\"req\",\"req\",0,[\"req\"]],[\"tx\",\"bubble\",3,\"tx\",\"req\",1,[\"tx\"]],[\"tx\",\"capture\",3,\"tx\",\"req\",1,[\"tx\"]],[\"db\",\"bubble\",3,\"db\",\"req\",2,[\"db\"]],[\"db\",\"capture\",3,\"db\",\"req\",2,[\"db\"]]]",
    "/path/stopped/settled = \"complete\"",
];

/// Listener exceptions abort the transaction like Chrome 156 (#148), whichever
/// listener of the request's event path threw, and only for that dispatch's own
/// listeners; the exception is reported at the window either way.
#[tokio::test(flavor = "current_thread")]
async fn indexeddb_events_match_chrome156() {
    let expected = control("indexeddb-events-probe.js", EVENTS_PROBE, EVENTS_CONTROL);
    assert_eq!(
        probe_divergences(EVENTS_PROBE, &expected).await,
        EVENTS_KNOWN_GAPS,
        "divergences from the Chrome control"
    );
}

/// One storage partition is one backing store: a same-origin child frame sees the
/// parent's database, and a StorageAccessHandle's `indexedDB` opens it too (the
/// top-level document's unpartitioned storage is its own partition).
#[tokio::test(flavor = "current_thread")]
async fn indexeddb_is_shared_within_a_storage_partition() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://idb-partition.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let got = page
        .eval_json_expression(
            r#"(async()=>{
      const opened=await new Promise(resolve=>{const request=indexedDB.open('shared',3);
        request.onupgradeneeded=()=>request.result.createObjectStore('s').put({v:'parent'},1);
        request.onsuccess=()=>resolve(request.result);});
      opened.close();
      const frame=document.createElement('iframe');document.body.append(frame);
      const child=frame.contentWindow;
      const listed=(await child.indexedDB.databases()).map(entry=>[entry.name,entry.version]);
      const value=await new Promise(resolve=>{const request=child.indexedDB.open('shared');
        request.onsuccess=()=>{const get=request.result.transaction('s').objectStore('s').get(1);
          get.onsuccess=()=>{request.result.close();resolve([get.result.v,get.result instanceof child.Object]);};};});
      const handle=await child.document.requestStorageAccess({indexedDB:true});
      const factory=handle.indexedDB;
      const viaHandle=(await factory.databases()).map(entry=>entry.name);
      return [listed,value,Object.prototype.toString.call(factory),factory===handle.indexedDB,viaHandle];
    })()"#,
        )
        .await
        .unwrap();
    page.close();
    assert_eq!(
        got,
        r#"[[["shared",3]],["parent",true],"[object IDBFactory]",true,["shared"]]"#
    );
}
