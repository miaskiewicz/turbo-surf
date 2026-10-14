use serde_json::{json, Value};
use turbo_surf_render::PageSession;

async fn page() -> PageSession {
    PageSession::open(
        "<!doctype html><iframe id=frame srcdoc='<body></body>'></iframe>",
        "https://message.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn window_capacity_failure_preserves_buffer_and_port_owners() {
    let mut page = page().await;
    page.set_window_message_limits_for_testing(8, 64);
    let raw=page.eval_json_expression("(()=>{const frame=document.getElementById('frame'),buffer=new ArrayBuffer(128),channel=new MessageChannel();let error=null;try{frame.contentWindow.postMessage({buffer,port:channel.port2},'*',[buffer,channel.port2]);}catch(caught){error=caught.name;}const port=structuredClone(channel.port2,{transfer:[channel.port2]});channel.port1.close();port.close();return {error,bytes:buffer.byteLength,port:port instanceof MessagePort};})()").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"error":"RangeError","bytes":128,"port":true})
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["queuedWindowMessages"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn window_recursive_getters_obey_the_current_count_budget() {
    let mut page = page().await;
    page.set_window_message_limits_for_testing(1, 4096);
    let raw=page.eval_json_expression("(()=>{const frame=document.getElementById('frame'),target=frame.contentWindow,outer=new ArrayBuffer(8),inner=new ArrayBuffer(8);let calls=0,error=null;try{target.postMessage({outer,get nested(){calls++;target.postMessage({inner},'*',[inner]);return true;}},'*',[outer]);}catch(caught){error=caught.name;}return {error,calls,outer:outer.byteLength,inner:inner.byteLength};})()").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"error":"RangeError","calls":1,"outer":8,"inner":0})
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["queuedWindowMessages"],
        1
    );
    page.eval("globalThis.__RESULT='';").await.unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["queuedWindowMessages"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn doomed_and_origin_mismatched_receivers_still_complete_transfers() {
    let mut page = page().await;
    page.set_window_message_limits_for_testing(0, 0);
    let raw=page.eval_json_expression("(()=>{const frame=document.getElementById('frame'),wrong=new ArrayBuffer(8),retired=new ArrayBuffer(8),target=frame.contentWindow;let wrongError=null,retiredError=null;try{target.postMessage(wrong,'https://other.test',[wrong]);}catch(error){wrongError=error.name;}try{target.postMessage({retired,get retire(){frame.remove();return true;}},'*',[retired]);}catch(error){retiredError=error.name;}return {wrongError,retiredError,wrong:wrong.byteLength,retired:retired.byteLength};})()").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"wrongError":null,"retiredError":null,"wrong":0,"retired":0})
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["queuedWindowMessages"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn shared_window_snapshots_use_fixed_byte_admission_and_release_after_delivery() {
    let headers = vec![
        ("Cross-Origin-Opener-Policy".into(), "same-origin".into()),
        ("Cross-Origin-Embedder-Policy".into(), "require-corp".into()),
    ];
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><iframe id=frame srcdoc='<body></body>'></iframe>",
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
    page.set_window_message_limits_for_testing(8, 65536);
    let raw=page.eval_json_expression("(()=>{const frame=document.getElementById('frame'),buffer=new ArrayBuffer(8),shared=new SharedArrayBuffer(65536);let error=null;try{frame.contentWindow.postMessage({shared,buffer},'*',[buffer]);}catch(caught){error=caught.name;}return {error,bytes:buffer.byteLength};})()").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"error":"RangeError","bytes":8})
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["queuedWindowMessages"],
        0
    );
    page.set_window_message_limits_for_testing(8, 1048576);
    page.eval_expression("(()=>{globalThis.memory=new WebAssembly.Memory({initial:1,maximum:2,shared:true});document.getElementById('frame').contentWindow.postMessage(memory.buffer,'*');memory.grow(1);return 'queued';})()").await.unwrap();
    let queued = page.frame_lifecycle_diagnostics_for_testing()["queuedWindowBytes"]
        .as_u64()
        .unwrap();
    assert!(
        (65536..65600).contains(&queued),
        "unexpected shared snapshot accounting: {queued}"
    );
    page.eval("globalThis.__RESULT='';").await.unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["queuedWindowBytes"],
        0
    );
}
