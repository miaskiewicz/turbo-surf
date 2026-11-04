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

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .ends_with("Chrome/156.0.8078.47"));
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(
        control["fixtures"]["indexeddb-probe.js"],
        format!("{:x}", Sha256::digest(PROBE)),
        "stale control"
    );
    control["result"]["indexeddb-probe.js"].clone()
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
async fn indexeddb_matches_chrome156() {
    let expected = control();
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
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let value: Value = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
    let mut found = Vec::new();
    divergences(String::new(), &actual, &expected, &mut found);
    assert_eq!(
        found,
        Vec::<String>::new(),
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
