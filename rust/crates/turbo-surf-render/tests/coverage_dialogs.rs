#[path = "support/popup_contract.rs"]
pub mod fixture;
use std::time::{Duration, Instant};
use turbo_surf_render::{DialogReply, NetworkEventLog};

#[tokio::test(flavor = "current_thread")]
async fn public_dialogs_without_listener_dismiss_and_preserve_conversion_errors() {
    let mut page = fixture::open("https://dialogs.test", Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let value = page
        .eval_target_expression(
            root,
            r#"(()=>{
      const calls=[]; const text={toString(){calls.push('message');return 'text'}};
      const alertValue=alert(text),confirmValue=confirm(text),promptValue=prompt(text,'default');
      let symbol=false,receiver=false;
      try{confirm(Symbol())}catch(e){symbol=e instanceof TypeError}
      try{prompt.call({},'x')}catch(e){receiver=e instanceof TypeError}
      return [alertValue===undefined,confirmValue,promptValue,calls,symbol,receiver];
    })()"#,
            false,
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        value,
        r#"[true,false,null,["message","message","message"],true,true]"#
    );
    page.close();
}

fn answer(
    log: NetworkEventLog,
    owner: String,
    text: Option<Vec<u16>>,
) -> std::thread::JoinHandle<serde_json::Value> {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let rows: Vec<serde_json::Value> = serde_json::from_str(&log.drain()).unwrap();
            for row in rows {
                if row["event"] == "dialog" {
                    let id = row["id"].as_str().unwrap().parse().unwrap();
                    log.dialogs()
                        .respond(
                            &owner,
                            id,
                            DialogReply {
                                accepted: true,
                                prompt_text: text,
                            },
                        )
                        .unwrap();
                    return row;
                }
            }
            assert!(
                Instant::now() < deadline,
                "dialog request was not published"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    })
}

#[tokio::test(flavor = "current_thread")]
async fn public_prompt_replies_preserve_utf16_and_target_ownership() {
    let mut page = fixture::open("https://dialogs.test", Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    for (target, reply, expected) in [
        (root, Some(vec![0xd800, 120]), "[55296,120]"),
        (peer, None, "[56320,121]"),
    ] {
        let log = page.network_event_log();
        log.dialogs().configure(&target.to_string(), true).unwrap();
        let worker = answer(log, target.to_string(), reply);
        let result = page.eval_target_expression(target, r#"Array.from(prompt(String.fromCharCode(0xd800),String.fromCharCode(0xdc00)+'y'), c=>c.charCodeAt(0))"#, false, true).await;
        let event = worker.join().unwrap();
        assert_eq!(result.unwrap(), expected);
        assert_eq!(event["messageUnits"], serde_json::json!([55296]));
        assert_eq!(event["defaultValueUnits"], serde_json::json!([56320, 121]));
    }
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn sandboxed_frame_dialogs_are_dismissed_without_host_publication() {
    let mut page = fixture::open("https://dialogs.test", Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    let log = page.network_event_log();
    log.dialogs().configure_context(true).unwrap();
    let result = page.eval_target_expression(root, r#"(async()=>{
      const frame=document.createElement('iframe');frame.sandbox='allow-scripts allow-same-origin';
      const ready=new Promise(resolve=>frame.onload=resolve);frame.srcdoc='<body>modal sandbox';document.body.append(frame);await ready;
      const child=frame.contentWindow;
      return [child.alert('blocked')===undefined,child.confirm('blocked'),child.prompt('blocked','default')];
    })()"#, false, true).await.unwrap();
    assert_eq!(result, "[true,false,null]");
    let events: Vec<serde_json::Value> = serde_json::from_str(&log.drain()).unwrap();
    assert!(!events.iter().any(|event| event["event"] == "dialog"));
    page.close();
}
