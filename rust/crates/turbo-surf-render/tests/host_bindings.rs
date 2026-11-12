use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::{
    BrowserStorage, DocumentResponseMetadata, HydrationPhase, PageSession, PageSessionOptions,
};

const HOST: &str = r#"
"use strict";
const bag=__tsNs.shimBindings ||= {__proto__:null,n:0,pending:{__proto__:null}};
globalThis.hostWait = () => {
  __tsNs.shimBindingPending(true);
  const id=++bag.n;
  return __tsNs.shimBindingPromise((resolve,reject) => {
    let done=false;
    const finish=()=>{if(!done){done=true;delete bag.pending[id];__tsNs.shimBindingPending(false);}};
    bag.pending[id]={resolve:value=>{finish();resolve(value);},reject:error=>{finish();reject(error);}};
    __tsNs.finishHost=value=>bag.pending[id].resolve(value);
  });
};
"#;

async fn page(html: &str) -> PageSession {
    PageSession::open_with_document_response_and_host_script(
        html,
        "http://127.0.0.1/",
        "",
        "",
        5000,
        "",
        ClientHints::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions::default(),
        HOST,
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn initial_module_host_dependency_yields_and_resumes_the_same_realm() {
    let mut page =
        page(r#"<body><script type="module">globalThis.result=await hostWait()</script>"#).await;
    assert_ne!(
        page.hydration_diagnostics().progress.phase,
        HydrationPhase::Complete
    );
    page.eval_engine("__tsNs.finishHost('ready'); globalThis.__RESULT='';")
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression("globalThis.result").await.unwrap(),
        "ready"
    );
    assert_eq!(
        page.hydration_diagnostics().progress.phase,
        HydrationPhase::Complete
    );
    assert_eq!(
        page.eval_expression("typeof __tsNs").await.unwrap(),
        "undefined"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retired_child_host_work_does_not_hold_the_owner_event_loop() {
    let mut page = page("<body><iframe srcdoc='<p>child</p>'></iframe>").await;
    page.eval_engine("globalThis.frames[0].hostWait(); globalThis.__RESULT='';")
        .await
        .unwrap();
    page.eval_engine("document.querySelector('iframe').remove(); globalThis.__RESULT='';")
        .await
        .unwrap();
    page.eval_engine("globalThis.afterRetired=[]; setTimeout(()=>afterRetired.push('timer'),0); globalThis.__RESULT='';").await.unwrap();
    assert_eq!(
        page.eval_expression("JSON.stringify(afterRetired)")
            .await
            .unwrap(),
        r#"["timer"]"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn guest_lookalike_queues_cannot_forge_a_host_dependency() {
    let result=PageSession::open(
      "<script>globalThis.__tsExpose={pending:new Map([[1,{}]])}</script><script type=module>await new Promise(()=>{})</script>",
      "http://127.0.0.1/", "", "", 5000,
    ).await;
    match result {
        Ok(_) => {
            panic!("a forged guest queue must not turn stalled module evaluation into a host yield")
        }
        Err(error) => assert!(
            error.contains("Top-level await") || error.contains("deadlock"),
            "{error}"
        ),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn late_driver_scripts_update_live_children_and_future_realms_without_rerunning_user_init() {
    let mut page = page("<body><iframe srcdoc='<p>existing</p>'></iframe>").await;
    let engine = "globalThis.driverMarker=()=>__tsNs.shimBindingId();";
    let guest =
        "globalThis.userInitCount=(globalThis.userInitCount||0)+1;globalThis.routeMarker='future';";
    let install = "globalThis.routeMarker='existing';";
    assert_eq!(page.eval_engine(&format!("__tsNs.shimBindingScripts({},{},{});globalThis.__RESULT=String(!!__tsNs.shimBindingBag(globalThis).pending);",serde_json::to_string(engine).unwrap(),serde_json::to_string(guest).unwrap(),serde_json::to_string(install).unwrap())).await.unwrap(),"true");
    let before:serde_json::Value=serde_json::from_str(&page.eval_expression("JSON.stringify([frames[0].driverMarker(),frames[0].routeMarker,frames[0].userInitCount||0,typeof globalThis.__tsNs])").await.unwrap()).unwrap();
    assert!(before[0].as_u64().unwrap() > 0);
    assert_eq!(before[1], "existing");
    assert_eq!(before[2], 0);
    assert_eq!(before[3], "undefined");
    page.eval_engine("const frame=document.createElement('iframe');frame.srcdoc='<p>future</p>';document.body.appendChild(frame);globalThis.__RESULT='';").await.unwrap();
    let after:serde_json::Value=serde_json::from_str(&page.eval_expression("JSON.stringify([frames[1].driverMarker(),frames[1].routeMarker,frames[1].userInitCount,frames[0].userInitCount||0])").await.unwrap()).unwrap();
    assert_ne!(before[0], after[0]);
    assert_eq!(after[1], "future");
    assert_eq!(after[2], 1);
    assert_eq!(after[3], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn post_load_host_dependency_yields_before_repeating_hydration_passes() {
    let mut page = page(
        r#"<body><script>
      addEventListener('load', () => setTimeout(() => {
        globalThis.postLoadResult = hostWait();
      }, 0));
    </script>"#,
    )
    .await;
    assert_eq!(
        page.hydration_diagnostics().progress.phase,
        HydrationPhase::PostLoadHydration
    );
    assert_eq!(
        page.hydration_diagnostics().progress.completed_pumps,
        0,
        "post-load hydration must yield as soon as an actual host callback is pending"
    );
    page.eval_engine("__tsNs.finishHost('ready');globalThis.__RESULT='';")
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression("postLoadResult").await.unwrap(),
        "ready"
    );
    assert_eq!(
        page.hydration_diagnostics().progress.phase,
        HydrationPhase::Complete
    );
}
