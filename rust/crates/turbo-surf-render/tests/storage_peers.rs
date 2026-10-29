use turbo_surf_render::{BrowserStorage, PageSession};

async fn open(storage: BrowserStorage, path: &str) -> PageSession {
    PageSession::open_with_browsing_state(
        "<!doctype html><body>",
        &format!("https://storage-peers.test/{path}"),
        "",
        "",
        10000,
        "",
        Default::default(),
        &[],
        storage,
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn queued_peer_mutations_exclude_later_realms_and_keep_handler_microtasks() {
    let profile = BrowserStorage::default();
    let mut source = open(profile.new_tab().unwrap(), "writer#source").await;
    let mut peer = open(profile.new_tab().unwrap(), "peer").await;
    peer.eval_expression("globalThis.received=[];addEventListener('storage',event=>{received.push([event.key,event.newValue,event.url,event.isTrusted,event.storageArea===localStorage]);queueMicrotask(()=>received.push('microtask'))})").await.unwrap();
    source
        .eval_expression("localStorage.setItem('queued','value')")
        .await
        .unwrap();
    assert!(peer.pending_storage_events_for_testing() > 0);
    peer.eval_expression("globalThis.lateCount=0;globalThis.lateFrame=document.createElement('iframe');document.body.append(lateFrame);lateFrame.contentWindow.addEventListener('storage',()=>lateCount++)").await.unwrap();
    peer.eval("globalThis.__RESULT='drained'").await.unwrap();
    assert_eq!(peer.eval_expression("JSON.stringify(received)").await.unwrap(),
        "[[\"queued\",\"value\",\"https://storage-peers.test/writer#source\",true,true],\"microtask\"]");
    assert_eq!(peer.eval_expression("lateCount").await.unwrap(), "0");
    assert_eq!(peer.pending_storage_events_for_testing(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn storage_event_signal_wakes_an_idle_host_when_a_peer_commits() {
    use std::time::Duration;
    let profile = BrowserStorage::default();
    let mut source = open(profile.new_tab().unwrap(), "signal-writer").await;
    let mut peer = open(profile.new_tab().unwrap(), "signal-peer").await;
    peer.eval_expression(
        "globalThis.keys=[];addEventListener('storage',event=>keys.push(event.key))",
    )
    .await
    .unwrap();
    // Nothing is queued: the idle signal stays pending.
    assert!(
        tokio::time::timeout(Duration::from_millis(20), peer.storage_event_signal())
            .await
            .is_err()
    );
    let signal = peer.storage_event_signal();
    source
        .eval_expression("localStorage.setItem('wake','1')")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), signal)
        .await
        .expect("a peer commit wakes the idle host driver");
    peer.eval("globalThis.__RESULT=''").await.unwrap();
    assert_eq!(peer.eval_expression("keys.join()").await.unwrap(), "wake");
    // Delivered: a fresh signal is pending again.
    assert!(
        tokio::time::timeout(Duration::from_millis(20), peer.storage_event_signal())
            .await
            .is_err()
    );
}

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn native_peer_inbox_payloads_do_not_root_retired_receiving_realms() {
    let profile = BrowserStorage::default();
    let mut source = open(profile.new_tab().unwrap(), "source").await;
    let mut peer = open(profile.new_tab().unwrap(), "receiver").await;
    peer.eval_expression("(()=>{const frame=document.createElement('iframe');document.body.append(frame);globalThis.peerFrame=frame;globalThis.peerRealm=new WeakRef(frame.contentWindow);return 'ready'})()").await.unwrap();
    source
        .eval_expression("localStorage.setItem('pending','value')")
        .await
        .unwrap();
    assert!(peer.pending_storage_events_for_testing() > 0);
    peer.eval_expression("peerFrame.remove();peerFrame=null")
        .await
        .unwrap();
    for _ in 0..3 {
        peer.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        peer.eval_expression("peerRealm.deref()===undefined")
            .await
            .unwrap(),
        "true"
    );
    assert!(peer.pending_storage_events_for_testing() > 0);
    peer.eval("globalThis.__RESULT='drained'").await.unwrap();
    assert_eq!(peer.pending_storage_events_for_testing(), 0);
    peer.close();
    source
        .eval("localStorage.setItem('after-close','value');globalThis.__RESULT='ok'")
        .await
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn nested_peer_dispatch_releases_each_queued_batch_before_guest_callbacks() {
    let profile = BrowserStorage::default();
    let mut source = open(profile.new_tab().unwrap(), "source").await;
    let mut peer = open(profile.new_tab().unwrap(), "receiver").await;
    peer.eval_engine_expression("globalThis.received=0;globalThis.nested=0;const frame=document.createElement('iframe');document.body.append(frame);frame.contentWindow.addEventListener('storage',()=>{received++;nested++});addEventListener('storage',()=>{received++;__tsNs.runTimers()})").await.unwrap();
    for round in 0..24 {
        source
            .eval_expression(&format!(
                "localStorage.setItem('large','{}'.repeat(1024*1024))",
                if round % 2 == 0 { "a" } else { "b" }
            ))
            .await
            .unwrap();
        peer.eval("globalThis.__RESULT='drained'").await.unwrap();
        assert_eq!(peer.pending_storage_events_for_testing(), 0);
    }
    assert_eq!(
        peer.eval_expression("JSON.stringify([received,nested])")
            .await
            .unwrap(),
        "[48,24]"
    );
    // Each mutation after the first costs roughly 4MiB. The old queued-byte
    // accounting leaked that charge whenever the root handler drained its child.
    peer.eval_expression("sessionStorage.setItem('after','x'.repeat(2*1024*1024))")
        .await
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn pending_peer_promise_receives_an_external_write_without_another_peer_operation() {
    let profile = BrowserStorage::default();
    let mut source = open(profile.new_tab().unwrap(), "source").await;
    let mut peer = open(profile.new_tab().unwrap(), "receiver").await;
    let waiting = peer.eval_expression(
        "new Promise(resolve=>addEventListener('storage',event=>resolve(event.key),{once:true}))",
    );
    let write = async {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        source
            .eval_expression("localStorage.setItem('wake','value')")
            .await
            .unwrap();
    };
    let (received, ()) = tokio::join!(waiting, write);
    assert_eq!(received.unwrap(), "wake");
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_peer_wait_does_not_block_a_later_operation_or_wait() {
    let profile = BrowserStorage::default();
    let mut source = open(profile.new_tab().unwrap(), "source").await;
    let mut peer = open(profile.new_tab().unwrap(), "receiver").await;
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(30),
        peer.eval_expression("new Promise(()=>{})")
    )
    .await
    .is_err());
    assert_eq!(peer.eval_expression("'alive'").await.unwrap(), "alive");
    let waiting = peer.eval_expression(
        "new Promise(resolve=>addEventListener('storage',event=>resolve(event.key),{once:true}))",
    );
    let write = async {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        source
            .eval_expression("localStorage.setItem('after-cancel','value')")
            .await
            .unwrap();
    };
    let (received, ()) = tokio::join!(waiting, write);
    assert_eq!(received.unwrap(), "after-cancel");
}

#[tokio::test(flavor = "current_thread")]
async fn pending_peer_tasks_use_native_dispatch_and_checkpoint_each_handler() {
    let profile = BrowserStorage::default();
    let mut source = open(profile.new_tab().unwrap(), "source").await;
    let mut peer = open(profile.new_tab().unwrap(), "receiver").await;
    let waiting = peer.eval_engine_expression(
        r#"new Promise(resolve=>{
      const environment=__tsNs, original=environment.runTimers, observed=[];
      environment.runTimers=()=>{throw new Error('public timer driver was called')};
      function receive(event){
        observed.push(event.key);
        queueMicrotask(()=>{
          observed.push('microtask:'+event.key);
          if(event.key==='second'){
            removeEventListener('storage',receive);
            environment.runTimers=original;
            resolve(JSON.stringify(observed));
          }
        });
      }
      addEventListener('storage',receive);
    })"#,
    );
    let write = async {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        source
            .eval_expression("localStorage.setItem('first','1');localStorage.setItem('second','2')")
            .await
            .unwrap();
    };
    let (received, ()) = tokio::join!(waiting, write);
    assert_eq!(
        received.unwrap(),
        r#"["first","microtask:first","second","microtask:second"]"#
    );
    assert_eq!(peer.pending_storage_events_for_testing(), 0);
}
