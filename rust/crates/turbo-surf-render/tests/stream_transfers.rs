use turbo_surf_render::PageSession;
#[tokio::test(flavor = "current_thread")]
async fn native_stream_transfers_match_independent_chrome_control() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://example.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/stream-transfer-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-transfer-chrome155.json"
    ))
    .unwrap();
    eprintln!("stream transfers: {actual}");
    assert_eq!(actual, control["result"]);
}
#[tokio::test(flavor = "current_thread")]
async fn native_stream_transfer_adversarial_chrome_controls() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://example.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/stream-adversarial-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-adversarial-chrome155.json"
    ))
    .unwrap();
    eprintln!("stream adversarial: {actual}");
    assert_eq!(actual, control["result"]);
}
#[tokio::test(flavor = "current_thread")]
async fn native_stream_transfer_retirement_matches_chrome() {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    struct Fixture(Child);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    use turbo_surf_core::{net, request_context::ClientHints};
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../harness/detection/upload-server.mjs")
        .canonicalize()
        .expect("stream fixture must exist in ordinary and linked test workspaces");
    let child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut fixture = Fixture(child);
    let mut line = String::new();
    BufReader::new(fixture.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(
        !line.is_empty(),
        "stream fixture exited before announcing its localhost endpoints"
    );
    let urls: serde_json::Value = serde_json::from_str(&line).unwrap();
    let hints = ClientHints::with_client_builder(|| {
        net::client_builder_with_trust(include_bytes!("csp_report_cert.der"))
    });
    let mut page = PageSession::open_with_request_state(
        "<!doctype html><body>",
        urls["primary"].as_str().unwrap(),
        "",
        "",
        20000,
        "",
        hints,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/stream-retirement-probe.js")
        .trim()
        .trim_end_matches(';');
    let result = page.eval_json_expression(probe).await;
    drop(fixture);
    let actual: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-retirement-chrome155.json"
    ))
    .unwrap();
    eprintln!("stream retirement: {actual}");
    assert_eq!(actual, control["result"]);
}
#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn completed_transfers_do_not_root_source_frame_realms() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://stream-gc.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    assert_eq!(page.eval_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const source=new child.ReadableStream({start(controller){controller.enqueue(7);controller.close();}});
      const target=structuredClone(source,{transfer:[source]}),reader=target.getReader();
      await reader.read();await reader.read();
      globalThis.streamWeakRefs=[new WeakRef(child),new WeakRef(source),new WeakRef(target),new WeakRef(reader)];
      frame.remove();return 'prepared';
    })()"#).await.unwrap(),"prepared");
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("JSON.stringify(streamWeakRefs.map(item=>item.deref()===undefined))")
            .await
            .unwrap(),
        "[true,true,true,true]"
    );
}
#[tokio::test(flavor = "current_thread")]
async fn trusted_transfer_steps_ignore_page_modified_port_and_stream_methods() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://stream-poison.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/stream-poison-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-poison-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn stream_cancel_context_matches_distinct_chrome_webidl_methods() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://stream-cancel.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/stream-cancel-context-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-cancel-context-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}
#[tokio::test(flavor = "current_thread")]
async fn stream_admission_failure_preserves_ownership_and_abandoned_plans_collect() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://stream-admission.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    page.set_message_port_limits_for_testing(0, 128);
    assert_eq!(page.eval_expression(r#"(()=>{
      const source=new ReadableStream(),buffer=new ArrayBuffer(8),channel=new MessageChannel();
      let name;try{channel.port1.postMessage({source,buffer},[source,buffer]);}catch(error){name=error.name;}
      channel.port1.close();channel.port2.close();return JSON.stringify([name,source.locked,buffer.byteLength]);
    })()"#).await.unwrap(),"[\"RangeError\",false,8]");
    page.set_message_port_limits_for_testing(100, 1024 * 1024);
    assert_eq!(page.eval_expression(r#"(()=>{
      for(let index=0;index<64;index++){
        const source=new ReadableStream(),buffer=new ArrayBuffer(8);
        try{structuredClone({source,method(){}},{transfer:[source,buffer]});throw new Error('accepted bad graph');}
        catch(error){if(error.name!=='DataCloneError')throw error;}
        if(source.locked||buffer.byteLength!==8)throw new Error('ownership changed');
      }return 'prepared';
    })()"#).await.unwrap(),"prepared");
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let state = page.message_port_diagnostics_for_testing();
    for name in ["endpoints", "queuedMessages", "queuedBytes", "reservations"] {
        assert_eq!(state[name], 0, "{state}");
    }
}
#[tokio::test(flavor = "current_thread")]
async fn source_pull_cannot_reenter_transfer_commit_and_lock_a_later_stream() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://stream-reentry.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/stream-reentry-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-reentry-chrome155.json"
    ))
    .unwrap();
    eprintln!("stream reentry: {actual}");
    assert_eq!(actual, control["result"]);
}
#[tokio::test(flavor = "current_thread")]
async fn source_pull_reentry_preserves_chrome_buffer_stream_port_transfer_phases() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://stream-order.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/stream-order-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-order-chrome155.json"
    ))
    .unwrap();
    eprintln!("stream phases: {actual}");
    assert_eq!(actual, control["result"]);
}
