//! A collected Promise parent keeps only a native terminal-state witness. An
//! unobserved/pending parent never becomes settled merely because it was collected.
use turbo_surf_render::PageSession;

fn collect_parent(page: &mut PageSession, root: u64) {
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
        if page
            .read_target_engine(root, "return parentWeak.deref()===undefined")
            .unwrap()
            == "true"
        {
            return;
        }
    }
    panic!("the unreferenced native parent Promise must be collected");
}

#[tokio::test(flavor = "current_thread")]
async fn collected_fulfilled_parent_still_runs_its_waiting_reaction() {
    let mut page = PageSession::open("<body>", "https://promise-parent.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    assert_eq!(page.read_target_engine(root, "const parent=Promise.resolve(41);globalThis.parentWeak=new WeakRef(parent);globalThis.observedChild=parent.then(value=>{globalThis.completed=value+1;return completed});return 'armed'").unwrap(), "\"armed\"");
    collect_parent(&mut page, root);
    assert_eq!(
        page.read_target_engine(root, "return globalThis.completed===undefined")
            .unwrap(),
        "true"
    );
    assert_eq!(
        page.eval_json_expression("observedChild").await.unwrap(),
        "42"
    );
    assert_eq!(page.eval_json_expression("completed").await.unwrap(), "42");
}

#[tokio::test(flavor = "current_thread")]
async fn collected_pending_parent_remains_unknown_instead_of_becoming_settled() {
    let mut page = PageSession::open("<body>", "https://promise-parent.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, "const parent=new Promise(()=>{});globalThis.parentWeak=new WeakRef(parent);globalThis.observedChild=parent.then(()=>{globalThis.completed=true});return 'armed'").unwrap();
    collect_parent(&mut page, root);
    let error = page
        .eval_json_expression("observedChild")
        .await
        .unwrap_err();
    assert!(
        error.contains("Native async work has unclassified ownership"),
        "{error}"
    );
    assert_eq!(
        page.read_target_engine(root, "return globalThis.completed===undefined")
            .unwrap(),
        "true"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn parent_settled_after_child_creation_survives_collection_before_checkpoint() {
    let mut page = PageSession::open("<body>", "https://promise-parent.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    assert_eq!(page.read_target_engine(root, "let complete;const parent=new Promise(resolve=>{complete=resolve});globalThis.parentWeak=new WeakRef(parent);globalThis.observedChild=parent.then(value=>{globalThis.completed=value+1;return completed});complete(41);complete=null;return 'armed'").unwrap(), "\"armed\"");
    collect_parent(&mut page, root);
    assert_eq!(
        page.read_target_engine(root, "return globalThis.completed===undefined")
            .unwrap(),
        "true"
    );
    assert_eq!(
        page.eval_json_expression("observedChild").await.unwrap(),
        "42"
    );
    assert_eq!(page.eval_json_expression("completed").await.unwrap(), "42");
}

#[tokio::test(flavor = "current_thread")]
async fn collected_pending_parent_does_not_block_an_owned_race_timer() {
    let mut page = PageSession::open("<body>", "https://promise-parent.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    // No resolver escapes this executor: this native parent remains pending.
    assert_eq!(page.read_target_engine(root, "const parent=new Promise(()=>{});globalThis.parentWeak=new WeakRef(parent);globalThis.observedChild=parent.then(()=>{globalThis.completed=true;return 'child'});globalThis.raceResult=Promise.race([observedChild,new Promise(resolve=>setTimeout(()=>resolve('timer'),10))]);return 'armed'").unwrap(), "\"armed\"");
    collect_parent(&mut page, root);
    assert_eq!(
        page.read_target_engine(root, "return globalThis.completed===undefined")
            .unwrap(),
        "true"
    );
    assert_eq!(
        page.eval_json_expression("raceResult").await.unwrap(),
        "\"timer\""
    );
    assert_eq!(page.eval_json_expression("6*7").await.unwrap(), "42");
    assert_eq!(
        page.eval_json_expression("new Promise(resolve=>setTimeout(()=>resolve(43),1))")
            .await
            .unwrap(),
        "43"
    );
    let error = page
        .eval_json_expression("observedChild")
        .await
        .unwrap_err();
    assert!(
        error.contains("Native async work has unclassified ownership"),
        "{error}"
    );
    assert_eq!(
        page.read_target_engine(root, "return globalThis.completed===undefined")
            .unwrap(),
        "true"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_peer_timer_does_not_authorize_a_collected_root_parent() {
    let mut page = PageSession::open("<body>", "https://promise-parent.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.eval_expression("window.open('about:blank','independent-timer');void 0")
        .await
        .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.source_page.as_deref() == Some(&root.to_string()))
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    page.read_target_engine(
        peer,
        "globalThis.peerFired=false;setTimeout(()=>{globalThis.peerFired=true},0);return 'armed'",
    )
    .unwrap();
    page.read_target_engine(root, "const parent=new Promise(()=>{});globalThis.parentWeak=new WeakRef(parent);globalThis.observedChild=parent.then(()=>{globalThis.completed=true});return 'armed'").unwrap();
    collect_parent(&mut page, root);
    let error = page
        .eval_json_expression("observedChild")
        .await
        .unwrap_err();
    assert!(
        error.contains("Native async work has unclassified ownership"),
        "{error}"
    );
    assert_eq!(
        page.read_target_engine(peer, "return peerFired").unwrap(),
        "false"
    );
    assert_eq!(
        page.read_target_engine(root, "return globalThis.completed===undefined")
            .unwrap(),
        "true"
    );
}
