//! Driver targets and own-result collectors share cooperative execution boundaries.
use serde_json::Value;
use turbo_surf_render::{
    BrowserStorage, DocumentResponseMetadata, PageSession, PageSessionOptions, WindowTimerMode,
};

async fn page(mode: WindowTimerMode) -> PageSession {
    PageSession::open_with_document_response_and_options(
        "<!doctype html><p>Target</p>",
        "https://target-budget.test/",
        "",
        "",
        100,
        "",
        Default::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions {
            window_timer_mode: mode,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}
const SETUP: &str = r#"
  globalThis.turns={entered:0,exited:0,active:false};
  globalThis.done=new Promise(resolve=>{
    const task=()=>{
      turns.entered++;turns.active=true;
      try {const end=performance.now()+70;while(performance.now()<end){}}
      finally {turns.active=false;turns.exited++;}
      if(turns.exited<6)setTimeout(task,0);else resolve('done');
    };
    setTimeout(task,0);
  });return true;
"#;
async fn check_target_engine(mode: WindowTimerMode) {
    let mut page = page(mode).await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, SETUP).unwrap();
    let result = page
        .eval_target_engine(root, "globalThis.__RESULT='reached';", false)
        .await;
    let state: Value =
        serde_json::from_str(&page.read_target_engine(root, "return turns;").unwrap()).unwrap();
    eprintln!("target-engine result={result:?}; turns={state}");
    assert!(
        state["entered"].as_u64().unwrap() > 0,
        "actual target callback must start"
    );
    assert_eq!(
        state["active"], false,
        "driver target returned with an interrupted stack: {state}"
    );
    assert_eq!(state["entered"], state["exited"]);
    assert_eq!(result.unwrap(), "reached");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let state: Value =
                serde_json::from_str(&page.read_target_engine(root, "return turns;")?).unwrap();
            if state["exited"] == 6 {
                break;
            }
            page.eval_target_engine(root, "globalThis.__RESULT='continue';", false)
                .await?;
        }
        Ok::<(), String>(())
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        page.read_target_engine(root, "return turns.exited;")
            .unwrap(),
        "6"
    );
    assert_eq!(
        page.eval_target_expression(root, "turns.entered", false, false)
            .await
            .unwrap(),
        "6"
    );
    page.close();
}
#[tokio::test(flavor = "current_thread")]
async fn driver_target_engine_preserves_healthy_callback_tails() {
    check_target_engine(WindowTimerMode::RealTime).await
}
#[tokio::test(flavor = "current_thread")]
async fn virtual_driver_target_engine_preserves_healthy_callback_tails() {
    check_target_engine(WindowTimerMode::Virtual).await
}

async fn check_expression(target: bool) {
    let mut page = page(WindowTimerMode::RealTime).await;
    let root = if target {
        page.auxiliary_root_id().unwrap()
    } else {
        0
    };
    if target {
        page.read_target_engine(root, SETUP).unwrap();
    } else {
        page.read_engine(SETUP).unwrap();
    }
    let result = if target {
        page.eval_target_expression(root, "done", false, false)
            .await
    } else {
        page.eval_expression("done").await
    };
    let state = if target {
        page.read_target_engine(root, "return turns;")
    } else {
        page.read_engine("return turns;")
    }
    .unwrap();
    let state: Value = serde_json::from_str(&state).unwrap();
    eprintln!("owned-expression target={target} result={result:?}; turns={state}");
    assert!(state["entered"].as_u64().unwrap() > 0);
    assert_eq!(
        state["active"], false,
        "own-result wait interrupted healthy callback: {state}"
    );
    assert_eq!(state["entered"], state["exited"]);
    assert_eq!(
        result.unwrap_err(),
        "Target expression exceeded session execution budget",
        "unfinished own promise retains its actual deadline error"
    );
    page.close();
}
#[tokio::test(flavor = "current_thread")]
async fn target_owned_expression_deadline_does_not_terminate_a_healthy_callback() {
    check_expression(true).await
}
#[tokio::test(flavor = "current_thread")]
async fn public_owned_expression_deadline_does_not_terminate_a_healthy_callback() {
    check_expression(false).await
}

#[tokio::test(flavor = "current_thread")]
async fn target_unbounded_callback_remains_an_explicit_hard_interruption() {
    let mut page = page(WindowTimerMode::RealTime).await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, "setTimeout(()=>{while(true){}},0);return true;")
        .unwrap();
    let error = page
        .eval_target_engine(root, "globalThis.__RESULT='reached';", false)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "Execution interrupted by the session execution budget"
    );
    page.close();
}

const HOST: &str = r#"
const bag=__tsNs.shimBindings ||= {__proto__:null,pending:{__proto__:null}};
globalThis.hostWait = () => {
  __tsNs.shimBindingPending(true);
  return __tsNs.shimBindingPromise(resolve => {
    bag.pending.actual=value=>{delete bag.pending.actual;__tsNs.shimBindingPending(false);resolve(value)};
    __tsNs.finishActualHost=value=>bag.pending.actual(value);
  });
};
"#;

#[tokio::test(flavor = "current_thread")]
async fn target_script_discovery_and_module_host_reply_survive_cooperative_handoffs() {
    let mut page = PageSession::open_with_document_response_and_host_script(
        "<!doctype html><p>Scripts</p>",
        "https://target-scripts.test/",
        "",
        "",
        1000,
        "",
        Default::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions::default(),
        HOST,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root,r#"const classic=document.createElement('script');classic.textContent="globalThis.classicRuns=(globalThis.classicRuns||0)+1";document.body.appendChild(classic);const module=document.createElement('script');module.type='module';module.textContent="globalThis.moduleReply=await hostWait()";document.body.appendChild(module);return true;"#).unwrap();
    page.eval_target_engine(root, "globalThis.__RESULT='handoff';", false)
        .await
        .unwrap();
    assert_eq!(
        page.read_target_engine(root, "return classicRuns;")
            .unwrap(),
        "1"
    );
    assert_eq!(
        page.read_target_engine(root, "return typeof moduleReply;")
            .unwrap(),
        "\"undefined\""
    );
    page.eval_target_engine(
        root,
        "__tsNs.finishActualHost('ready');globalThis.__RESULT='replied';",
        true,
    )
    .await
    .unwrap();
    page.eval_target_engine(root, "globalThis.__RESULT='finished';", false)
        .await
        .unwrap();
    assert_eq!(
        page.read_target_engine(root, "return moduleReply;")
            .unwrap(),
        "\"ready\""
    );
    assert_eq!(
        page.read_target_engine(root, "return classicRuns;")
            .unwrap(),
        "1"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn a_rejected_target_script_pass_is_reported_instead_of_discarded() {
    let mut page = page(WindowTimerMode::RealTime).await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(
        root,
        "__tsNs.hydrate=async()=>{throw new Error('actual script pass refused')};return true;",
    )
    .unwrap();
    let error = page
        .eval_target_engine(root, "globalThis.__RESULT='started';", false)
        .await
        .unwrap_err();
    assert!(error.contains("actual script pass refused"), "{error}");
    page.close();
}
