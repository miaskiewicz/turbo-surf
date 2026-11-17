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
