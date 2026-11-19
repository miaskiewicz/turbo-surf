//! IndexedDB (open/upgradeneeded/versionchange, stores, indexes, cursors, key
//! ranges, structured clone, auto-commit, abort, errors, key order) against the
//! source-pinned Chrome 156 control,
//! offline over an empty document.
use serde_json::{json, Value};
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
/// (#193, split from #148; pinned exactly so a fix or a new divergence fails here).
///
/// Items 1 (`/path/full` + `/microtasks`) and 2 (per-listener checkpoint) landed with
/// turbo-dom#381/#382 and surf's indexeddb.js switch to the engine-walked event path
/// (parent hook on request -> transaction -> database; one event; per-listener
/// microtask checkpoint from the binding).
///
/// `/deactivation`: Chrome deactivates a new transaction at the end of its task, so a
/// timer queued earlier in that task sees `TransactionInactiveError`; surf still does
/// so in a 0 ms timer. Blocked on turbo-dom#383 (end-of-task / microtasks-completed
/// callback; needs a new rusty_v8 binding, deferred to v0.9.0).
/// `/path/stopped/log` + `/path/stopped/settled` closed by turbo-dom#433 on
/// release/0.8.2 (dispatchMulti now short-circuits AT_TARGET on stopPropagation too,
/// not just stopImmediate). `/deactivation` remains blocked on turbo-dom#383.
const EVENTS_KNOWN_GAPS: &[&str] = &["/deactivation/1/1 = \"ok\""];

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

const PARTITION_PROBE: &str =
    include_str!("../../../../harness/detection/indexeddb-partition-probe.js");
const PARTITION_WIDGET: &str =
    include_str!("../../../../harness/detection/indexeddb-partition-widget.js");
const PARTITION_CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/indexeddb-partition-chrome156.json");

/// Serves the partition control's frames on localhost: `/idb-widget?…` runs the widget,
/// `/idb-middle?…` embeds it from 127.0.0.1 (the A -> B -> A chain).
async fn serve_partition_frames(listener: tokio::net::TcpListener) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let port = listener.local_addr().unwrap().port();
    while let Ok((mut socket, _)) = listener.accept().await {
        tokio::spawn(async move {
            let mut bytes = [0; 8192];
            let count = socket.read(&mut bytes).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&bytes[..count]);
            let path = head.split(' ').nth(1).unwrap_or("/");
            let query = path.split_once('?').map_or("", |(_, query)| query);
            let body = if path.starts_with("/idb-middle?") {
                format!("<iframe src=\"http://127.0.0.1:{port}/idb-widget?{query}\"></iframe><script>addEventListener('message',event=>parent.postMessage(event.data,'*'))</script>")
            } else {
                format!("<!doctype html><script>{PARTITION_WIDGET}</script>")
            };
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = socket.write_all(response.as_bytes()).await;
        });
    }
}

/// IndexedDB is partitioned like Chrome 156 (#148): a cross-site 127.0.0.1 frame under a
/// localhost page, and the A -> B -> A frame under a 127.0.0.1 page, each get their own
/// databases, a same-origin frame shares the page's, and a StorageAccessHandle's
/// `indexedDB` opens the frame origin's unpartitioned databases. Headless shell denies
/// requestStorageAccess, so the control is the frozen branded Chrome 156 build.
#[tokio::test(flavor = "current_thread")]
async fn indexeddb_partitions_match_chrome156() {
    let control: Value = serde_json::from_str(PARTITION_CONTROL).unwrap();
    assert_eq!(control["browser"], "Chrome/156.0.8078.80");
    for (key, source) in [
        ("probeSha256", PARTITION_PROBE),
        ("widgetSha256", PARTITION_WIDGET),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", Sha256::digest(source)),
            "stale {key}"
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(serve_partition_frames(listener));
    let storage = turbo_surf_render::BrowserStorage::default();
    let probe = PARTITION_PROBE.trim().trim_end_matches(';');
    let mut actual = Vec::new();
    for host in ["localhost", "127.0.0.1"] {
        let mut page = PageSession::open_with_browsing_state(
            "<!doctype html><body>IndexedDB partition control",
            &format!("http://{host}:{port}/"),
            "",
            "",
            15000,
            "",
            turbo_surf_core::request_context::ClientHints::default(),
            &[],
            storage.clone(),
        )
        .await
        .unwrap();
        let result = page
            .eval(&format!("(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})().catch(e=>{{globalThis.__RESULT=JSON.stringify({{error:String(e),stack:e.stack}});}})"))
            .await
            .unwrap();
        page.close();
        actual.push(serde_json::from_str::<Value>(&result).unwrap());
    }
    server.abort();
    assert_eq!(Value::Array(actual), control["result"]);
}

/// #193 item 4: removing the first opener must release every other guest realm.
/// The required infrastructure context is counted separately from the actual root.
#[tokio::test(flavor = "current_thread")]
async fn removed_first_opener_frame_releases_its_realm() {
    let mut page = PageSession::open(
        "<!doctype html><body><iframe id=f></iframe>",
        "https://idb-lifetime.test/",
        "",
        "",
        20_000,
    )
    .await
    .unwrap();
    page.eval_json_expression("(globalThis.__idbRootIdentity = { crypto, document }, true)")
        .await
        .unwrap();
    // The child still opens IDB before the root or any peer does.
    page.eval_json_expression(
        r#"(async()=>{
          const child=document.getElementById('f').contentWindow;
          await new Promise(resolve=>{const request=child.indexedDB.open('leaked',1);
            request.onupgradeneeded=()=>request.result.createObjectStore('s');
            request.onsuccess=()=>{request.result.close();resolve();};});
          document.getElementById('f').remove();
          return 'removed';
        })()"#,
    )
    .await
    .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let contexts = page.context_registry_diagnostics_for_testing();
    assert_eq!(contexts["guestRootKnown"], true, "{contexts}");
    assert_eq!(contexts["liveGuestRoot"], 1, "{contexts}");
    assert_eq!(contexts["liveOtherGuest"], 0, "{contexts}");
    assert_eq!(contexts["liveInfrastructure"], 1, "{contexts}");
    let roles = ["liveGuestRoot", "liveOtherGuest", "liveInfrastructure"]
        .iter()
        .map(|role| contexts[*role].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(contexts["live"].as_u64(), Some(roles), "{contexts}");
    let root_unchanged = page
        .eval_json_expression(
            "crypto === __idbRootIdentity.crypto && document === __idbRootIdentity.document",
        )
        .await
        .unwrap();
    assert_eq!(root_unchanged, "true");
}

/// Committed values must outlive their creator without carrying its realm's constructors.
#[tokio::test(flavor = "current_thread")]
async fn committed_data_survives_first_opener_retirement_in_peer_realm() {
    let mut page = PageSession::open(
        "<!doctype html><body><iframe id=first></iframe><iframe id=peer></iframe>",
        "https://idb-persistent-peer.test/",
        "",
        "",
        20_000,
    )
    .await
    .unwrap();
    let result = page
        .eval_json_expression(
            r#"(async()=>{
      const first=document.getElementById('first').contentWindow;
      const peer=document.getElementById('peer').contentWindow;
      const open=(w)=>new Promise((resolve,reject)=>{
        const r=w.indexedDB.open('persistent',1);
        r.onupgradeneeded=()=>r.result.createObjectStore('values');
        r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);
      });
      const done=(tx)=>new Promise((resolve,reject)=>{
        tx.oncomplete=resolve;tx.onabort=()=>reject(tx.error);
      });
      const db=await open(first);
      const tx=db.transaction('values','readwrite');
      const buffer=new first.ArrayBuffer(4);
      const view=new first.Uint8Array(buffer);view[0]=37;
      tx.objectStore('values').put({date:new first.Date(1234),buffer,view},'value');
      await done(tx);db.close();
      document.getElementById('first').remove();
      const other=await open(peer);
      const read=()=>new Promise((resolve,reject)=>{
        const r=other.transaction('values').objectStore('values').get('value');
        r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);
      });
      const value=await read();
      const checks=[value.date instanceof peer.Date,value.date.getTime()===1234,
        value.buffer instanceof peer.ArrayBuffer,value.view instanceof peer.Uint8Array,
        value.view.buffer===value.buffer,value.view[0]===37];
      value.view[0]=99;
      const again=await read();checks.push(again.view[0]===37);
      other.close();return checks;
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        serde_json::json!([true, true, true, true, true, true, true])
    );
}

/// Removing an owner must release its connection without losing a peer's blocked upgrade.
#[tokio::test(flavor = "current_thread")]
async fn retired_connection_releases_healthy_peer_upgrade() {
    let mut page = PageSession::open(
        "<!doctype html><body><iframe id=first></iframe><iframe id=peer></iframe>",
        "https://idb-blocked-retirement.test/",
        "",
        "",
        20_000,
    )
    .await
    .unwrap();
    let result = page
        .eval_json_expression(
            r#"(async()=>{
      const first=document.getElementById('first').contentWindow;
      const peer=document.getElementById('peer').contentWindow;
      await new Promise((resolve,reject)=>{
        const r=first.indexedDB.open('blocked',1);
        r.onupgradeneeded=()=>r.result.createObjectStore('values');
        r.onsuccess=resolve;r.onerror=()=>reject(r.error);
      });
      let blocked;
      const observed=new Promise(resolve=>blocked=resolve);
      const result=new Promise((resolve,reject)=>{
        const r=peer.indexedDB.open('blocked',2);
        r.onblocked=blocked;
        r.onupgradeneeded=()=>r.result.createObjectStore('second');
        r.onsuccess=()=>{const db=r.result;const version=db.version;
          const names=Array.from(db.objectStoreNames);db.close();resolve({version,names});};
        r.onerror=()=>reject(r.error);
      });
      await observed;document.getElementById('first').remove();
      return await result;
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        serde_json::json!({"version":2,"names":["second","values"]})
    );
}

/// Failed writes and schema upgrades must never publish their temporary native snapshot.
#[tokio::test(flavor = "current_thread")]
async fn aborted_changes_remain_invisible_to_a_peer_realm() {
    let mut page = PageSession::open(
        "<!doctype html><body><iframe id=writer></iframe><iframe id=reader></iframe>",
        "https://idb-atomic-peer.test/",
        "",
        "",
        20_000,
    )
    .await
    .unwrap();
    let result = page.eval_json_expression(r#"(async()=>{
      const writer=document.getElementById('writer').contentWindow;
      const reader=document.getElementById('reader').contentWindow;
      const open=(w,version)=>new Promise((resolve,reject)=>{
        const r=w.indexedDB.open('atomic',version);
        r.onupgradeneeded=()=>r.result.createObjectStore('values');
        r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);
      });
      const finished=(tx,aborted)=>new Promise((resolve,reject)=>{
        tx.oncomplete=()=>aborted?reject(Error('unexpected commit')):resolve();
        tx.onabort=()=>aborted?resolve():reject(tx.error);
      });
      const db=await open(writer,1);
      const initial=db.transaction('values','readwrite');
      initial.objectStore('values').put('committed','key');await finished(initial,false);
      const aborted=db.transaction('values','readwrite');
      const put=aborted.objectStore('values').put('temporary','key');
      put.onsuccess=()=>aborted.abort();await finished(aborted,true);db.close();
      await new Promise((resolve,reject)=>{
        const r=writer.indexedDB.open('atomic',2);
        r.onupgradeneeded=()=>{r.result.deleteObjectStore('values');
          r.result.createObjectStore('temporary');r.transaction.abort();};
        r.onerror=e=>{e.preventDefault();resolve();};r.onsuccess=()=>reject(Error('upgrade committed'));
      });
      const peer=await open(reader);
      const value=await new Promise((resolve,reject)=>{
        const r=peer.transaction('values').objectStore('values').get('key');
        r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);
      });
      const result={version:peer.version,names:Array.from(peer.objectStoreNames),value};
      peer.close();return result;
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        serde_json::json!({"version":1,"names":["values"],"value":"committed"})
    );
}

/// Reentrant removal must abandon the writer and resume an already-blocked peer read.
#[tokio::test(flavor = "current_thread")]
async fn retiring_a_writer_inside_its_success_listener_preserves_committed_data() {
    let mut page = PageSession::open(
        "<!doctype html><iframe id=first></iframe><iframe id=peer></iframe>",
        "https://idb-reentrant-retirement.test/",
        "",
        "",
        20_000,
    )
    .await
    .unwrap();
    let result = page
        .eval_json_expression(
            r#"(async()=>{
      const first=document.getElementById('first').contentWindow;
      const peer=document.getElementById('peer').contentWindow;
      const open=w=>new Promise((resolve,reject)=>{
        const r=w.indexedDB.open('retired-writer',1);
        r.onupgradeneeded=()=>r.result.createObjectStore('values');
        r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);
      });
      const db=await open(first);
      const initial=db.transaction('values','readwrite');
      initial.objectStore('values').put('committed','key');
      await new Promise((resolve,reject)=>{
        initial.oncomplete=resolve;initial.onabort=()=>reject(initial.error);
      });
      const other=await open(peer);
      const result=await new Promise((resolve,reject)=>{
        const abandoned=db.transaction('values','readwrite');
        const write=abandoned.objectStore('values').put('abandoned','key');
        write.onerror=()=>reject(write.error);
        write.onsuccess=()=>{
          const read=other.transaction('values').objectStore('values').get('key');
          read.onsuccess=()=>resolve(read.result);read.onerror=()=>reject(read.error);
          document.getElementById('first').remove();
        };
      });
      other.close();return result;
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!("committed")
    );
}

/// A real root fetch must not consume a different Page's IDB continuation.
#[tokio::test(flavor = "current_thread")]
async fn root_navigation_preserves_a_peers_pending_idb_continuation() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for expected in ["/peer", "/next"] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut bytes = [0; 1024];
                let size = socket.read(&mut bytes).await.unwrap();
                assert!(size > 0 && header.len() + size <= 8192);
                header.extend_from_slice(&bytes[..size]);
            }
            let first_line = format!("GET {expected} HTTP/1.1\r\n");
            assert!(String::from_utf8_lossy(&header).starts_with(&first_line));
            let body = if expected == "/peer" {
                "<!doctype html><p>peer ready</p>"
            } else {
                "<!doctype html><p>root replacement</p>"
            };
            let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{body}",body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
            requests.push(expected.to_owned());
        }
        requests
    });
    let mut page = PageSession::open(
        "<iframe id=first></iframe>",
        &format!("{origin}/start"),
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_json_expression(
        r#"(async()=>{
      const first=document.getElementById('first').contentWindow;
      await new Promise((resolve,reject)=>{
        const r=first.indexedDB.open('peer-scope',1);
        r.onupgradeneeded=()=>r.result.createObjectStore('values');
        r.onsuccess=resolve;r.onerror=()=>reject(r.error);
      });
      window.open('about:blank','idb-peer');return true;
    })()"#,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let peer: u64 = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|entry| entry.target != root.to_string())
        .unwrap()
        .target
        .parse()
        .unwrap();
    // A real same-origin document provides the same native storage key; an
    // about:blank URL is not a reliable fixture for top-level partitioning.
    page.navigate_target(peer, &format!("{origin}/peer"), "push")
        .await
        .unwrap();
    page.eval_target_expression(
        peer,
        r#"(async()=>{
      globalThis.upgradeEvents=0;
      let blocked;const observed=new Promise(resolve=>blocked=resolve);
      globalThis.outcome=new Promise(resolve=>{
        const r=indexedDB.open('peer-scope',2);r.onblocked=blocked;
        r.onupgradeneeded=()=>{upgradeEvents++;throw Error('peer-upgrade-only');};
        r.onerror=e=>{e.preventDefault();resolve(r.error.name);};
        r.onsuccess=()=>{r.result.close();resolve('unexpected-success');};
      });
      await observed;return true;
    })()"#,
        false,
        true,
    )
    .await
    .unwrap();
    let action=format!("document.getElementById('first').remove();location.href='{origin}/next';globalThis.__RESULT='root-ok';");
    assert_eq!(
        page.dispatch_target_engine(root, &action).await.unwrap(),
        "root-ok"
    );
    assert_eq!(
        server.await.unwrap(),
        vec!["/peer".to_owned(), "/next".to_owned()]
    );
    let pending = page.indexeddb_diagnostics_for_testing();
    let tasks = pending["pendingContinuations"].as_array().unwrap();
    assert_eq!(tasks.len(), 1, "{pending}");
    assert_eq!(tasks[0]["target"], peer, "{pending}");
    assert_eq!(
        page.read_target_engine(peer, "return globalThis.upgradeEvents;")
            .unwrap(),
        "0"
    );
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("root replacement"));
    assert_eq!(
        page.eval_target_expression(peer, "globalThis.outcome", false, false)
            .await
            .unwrap(),
        "AbortError"
    );
    assert_eq!(
        page.read_target_engine(peer, "return globalThis.upgradeEvents;")
            .unwrap(),
        "1"
    );
    assert_eq!(
        page.indexeddb_diagnostics_for_testing()["pendingContinuations"],
        json!([])
    );
    assert!(page
        .serialize_target(root)
        .unwrap()
        .contains("root replacement"));
}

/// Chrome 156 rejects storage-only clone types before creating a request, without aborting the transaction.
#[tokio::test(flavor = "current_thread")]
async fn storage_clone_errors_are_synchronous_and_leave_the_transaction_usable() {
    let headers = vec![
        ("Cross-Origin-Opener-Policy".into(), "same-origin".into()),
        ("Cross-Origin-Embedder-Policy".into(), "require-corp".into()),
    ];
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body></body>",
        "http://localhost/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let raw = page.eval_json_expression(r#"(async()=>{
      const request=indexedDB.open('storage-clone-policy',1);
      const db=await new Promise((resolve,reject)=>{
        request.onupgradeneeded=()=>request.result.createObjectStore('s');
        request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);
      });
      const bytes=[0,97,115,109,1,0,0,0,1,5,1,96,0,1,127,3,2,1,0,7,10,1,6,97,110,115,119,101,114,0,0,10,6,1,4,0,65,42,11];
      const module=new WebAssembly.Module(new Uint8Array(bytes));
      const answer=new WebAssembly.Instance(module).exports.answer();
      const genericCloneAnswer=new WebAssembly.Instance(structuredClone(module)).exports.answer();
      const values=[['module',module],['shared',new SharedArrayBuffer(4)]];
      const rows=[];
      for(const [name,value] of values){
        const tx=db.transaction('s','readwrite'),store=tx.objectStore('s');
        let error,operation;
        try{operation=store.put(value,name);}catch(caught){
          error={name:caught.name,message:caught.message,domException:caught instanceof DOMException};
        }
        if(!error){
          operation.onerror=event=>event.preventDefault();
          const stopped=new Promise(resolve=>tx.onabort=resolve);
          tx.abort();await stopped;
          rows.push({name,synchronous:false});continue;
        }
        store.put('healthy',`after-${name}`);
        const transaction=await new Promise(resolve=>{
          tx.oncomplete=()=>resolve('complete');tx.onabort=()=>resolve(`abort:${tx.error?.name}`);
        });
        const after=await new Promise((resolve,reject)=>{
          const read=db.transaction('s').objectStore('s').get(`after-${name}`);
          read.onsuccess=()=>resolve(read.result);read.onerror=()=>reject(read.error);
        });
        rows.push({name,synchronous:true,error,requestCreated:operation!==undefined,transaction,after});
      }
      db.close();return {answer,genericCloneAnswer,crossOriginIsolated,rows};
    })()"#).await.unwrap();
    let result: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(result["answer"], 42);
    assert_eq!(result["genericCloneAnswer"], 42);
    assert_eq!(result["crossOriginIsolated"], true);
    for (index, type_name) in ["WebAssembly.Module", "SharedArrayBuffer"]
        .iter()
        .enumerate()
    {
        let row = &result["rows"][index];
        assert_eq!(
            row["synchronous"], true,
            "storage validation must reject before creating a request: {row}"
        );
        assert_eq!(row["error"]["name"], "DataCloneError");
        assert_eq!(row["error"]["domException"], true);
        assert_eq!(row["error"]["message"], format!("Failed to execute 'put' on 'IDBObjectStore': A {type_name} can not be serialized for storage."));
        assert_eq!(row["requestCreated"], false);
        assert_eq!(row["transaction"], "complete");
        assert_eq!(row["after"], "healthy");
    }
}

async fn open_pending_upgrade_peer(page: &mut PageSession, name: &str, url: &str) -> u64 {
    let previous: Vec<_> = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .map(|entry| entry.target)
        .collect();
    let name = serde_json::to_string(name).unwrap();
    page.eval_expression(&format!("window.open('',{name});true"))
        .await
        .unwrap();
    let target = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|entry| !previous.contains(&entry.target))
        .unwrap()
        .target
        .parse()
        .unwrap();
    page.navigate_target(target, url, "push").await.unwrap();
    page.eval_target_expression(
        target,
        &format!(
            r#"(async()=>{{
        globalThis.upgradeEvents=0;
        let blocked;const observed=new Promise(resolve=>blocked=resolve);
        globalThis.outcome=new Promise(resolve=>{{
          const r=indexedDB.open({name},2);r.onblocked=blocked;
          r.onupgradeneeded=()=>{{upgradeEvents++;throw Error('selected-upgrade');}};
          r.onerror=e=>{{e.preventDefault();resolve(r.error.name);}};
          r.onsuccess=()=>{{r.result.close();resolve('unexpected-success');}};
        }});
        await observed;return true;
      }})()"#
        ),
        false,
        true,
    )
    .await
    .unwrap();
    target
}

enum RootPeerDependency {
    Await,
    LegacyThen,
    LegacyAll,
}

async fn check_root_peer_dependency(mode: RootPeerDependency) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for expected in ["/dependent", "/unrelated"] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut bytes = [0; 1024];
                let size = socket.read(&mut bytes).await.unwrap();
                assert!(size > 0 && header.len() + size <= 8192);
                header.extend_from_slice(&bytes[..size]);
            }
            assert!(String::from_utf8_lossy(&header)
                .starts_with(&format!("GET {expected} HTTP/1.1\r\n")));
            let body = "<!doctype html><p>owned peer</p>";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{body}",body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
            requests.push(expected.to_owned());
        }
        requests
    });
    let mut page = PageSession::open(
        "<iframe id=dependent></iframe><iframe id=unrelated></iframe>",
        &format!("{origin}/start"),
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_json_expression(
        r#"(async()=>{
      for(const name of ['dependent','unrelated']){
        const child=document.getElementById(name).contentWindow;
        await new Promise((resolve,reject)=>{
          const r=child.indexedDB.open(name,1);
          r.onupgradeneeded=()=>r.result.createObjectStore('values');
          r.onsuccess=resolve;r.onerror=()=>reject(r.error);
        });
      }
      return true;
    })()"#,
    )
    .await
    .unwrap();
    let dependent =
        open_pending_upgrade_peer(&mut page, "dependent", &format!("{origin}/dependent")).await;
    let unrelated =
        open_pending_upgrade_peer(&mut page, "unrelated", &format!("{origin}/unrelated")).await;
    let root = page.auxiliary_root_id().unwrap();
    assert_eq!(
        page.dispatch_target_engine(root, "document.querySelectorAll('iframe').forEach(frame=>frame.remove());globalThis.__RESULT='root-void';")
            .await.unwrap(),
        "root-void"
    );
    let pending = page.indexeddb_diagnostics_for_testing();
    let tasks = pending["pendingContinuations"].as_array().unwrap();
    assert_eq!(tasks.len(), 2, "{pending}");
    for target in [dependent, unrelated] {
        assert!(
            tasks.iter().any(|task| task["target"] == target),
            "{pending}"
        );
        assert_eq!(
            page.read_target_engine(target, "return globalThis.upgradeEvents;")
                .unwrap(),
            "0"
        );
    }
    let result = match mode {
        RootPeerDependency::Await => page
            .eval_target_expression(
                root,
                "(async()=>await window.open('', 'dependent').outcome)()",
                false,
                false,
            )
            .await
            .unwrap(),
        RootPeerDependency::LegacyThen => page
            .eval("globalThis.__RESULT='pending';void window.open('', 'dependent').outcome.then(value=>{globalThis.__RESULT=value;});void 0")
            .await
            .unwrap(),
        RootPeerDependency::LegacyAll => page
            .eval("globalThis.__RESULT='pending';void Promise.all([window.open('', 'dependent').outcome]).then(values=>{globalThis.__RESULT=values[0];});void 0")
            .await
            .unwrap(),
    };
    assert_eq!(result, "AbortError");
    let pending = page.indexeddb_diagnostics_for_testing();
    let tasks = pending["pendingContinuations"].as_array().unwrap();
    assert_eq!(tasks.len(), 1, "{pending}");
    assert_eq!(tasks[0]["target"], unrelated, "{pending}");
    assert_eq!(
        page.read_target_engine(unrelated, "return globalThis.upgradeEvents;")
            .unwrap(),
        "0"
    );
    assert_eq!(
        page.eval_target_expression(unrelated, "globalThis.outcome", false, false)
            .await
            .unwrap(),
        "AbortError"
    );
    assert_eq!(
        page.indexeddb_diagnostics_for_testing()["pendingContinuations"],
        json!([])
    );
    assert_eq!(server.await.unwrap(), vec!["/dependent", "/unrelated"]);
}

#[tokio::test(flavor = "current_thread")]
async fn root_awaits_its_peer_idb_dependency_without_running_an_unrelated_peer() {
    check_root_peer_dependency(RootPeerDependency::Await).await;
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_root_then_completes_its_preexisting_peer_idb_dependency_only() {
    check_root_peer_dependency(RootPeerDependency::LegacyThen).await;
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_root_promise_all_completes_its_preexisting_peer_idb_dependency_only() {
    check_root_peer_dependency(RootPeerDependency::LegacyAll).await;
}
