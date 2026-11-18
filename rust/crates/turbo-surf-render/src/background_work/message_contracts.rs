//! The public background turn services messages produced by real child scripts.
use super::*;
use serde_json::json;

fn post_from_actual_child(page: &mut PageSession) {
    let _activation = page.rt.activation_key().enter();
    let root = private_infrastructure::context(&mut page.rt).unwrap();
    let contexts = page
        .rt
        .op_state()
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    let child = {
        deno_core::scope!(scope, page.rt);
        contexts
            .into_iter()
            .find(|candidate| {
                let context = v8::Local::new(scope, candidate);
                let Some(host) = context
                    .get_slot::<FrameHost>()
                    .and_then(|host| host.0.to_local(scope))
                else {
                    return false;
                };
                crate::browser_env::node_attribute(scope, host, "id").as_deref() == Some("child")
            })
            .expect("the actual child frame must exist")
    };
    let listener = r#"globalThis.deliveries=[];addEventListener('message',event=>deliveries.push({data:event.data,origin:event.origin,source:event.source===document.querySelector('#child').contentWindow,trusted:event.isTrusted}));"#;
    assert!(execute_frame_script(
        &mut page.rt,
        root,
        "https://background-message.test/root-fixture.js",
        &listener.encode_utf16().collect::<Vec<_>>(),
    )
    .is_ok());
    assert!(execute_frame_script(
        &mut page.rt,
        child,
        "https://background-message.test/child-fixture.js",
        &"parent.postMessage({n:7}, '*');"
            .encode_utf16()
            .collect::<Vec<_>>(),
    )
    .is_ok());
    assert_eq!(
        page.rt
            .op_state()
            .borrow()
            .borrow::<FrameContexts>()
            .messages
            .len(),
        1,
        "the real child postMessage must produce the native handoff task"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn background_turn_services_an_actual_cross_frame_message_handoff() {
    let mut page = PageSession::open_with_document_response_and_options(
        "<body><p id=stable>healthy</p><iframe id=child srcdoc='<body>'></iframe>",
        "https://background-message.test/root",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        page.rt.op_state().borrow().borrow::<TimerClock>().mode,
        WindowTimerMode::RealTime
    );
    post_from_actual_child(&mut page);
    assert_eq!(
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            page.poll_background_work()
        )
        .await
        .expect("an already queued native task must make progress")
        .unwrap(),
        ""
    );
    assert!(page
        .rt
        .op_state()
        .borrow()
        .borrow::<FrameContexts>()
        .messages
        .is_empty());
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        page.poll_background_work(),
    )
    .await
    .expect("the handed-off posted-message task must make progress")
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &page
            .eval_json_expression("({deliveries,stable:document.querySelector('#stable').textContent,privateBindings:[typeof __tsNs,typeof Deno]})")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        actual,
        json!({"deliveries":[{"data":{"n":7},"origin":"https://background-message.test","source":true,"trusted":true}],"stable":"healthy","privateBindings":["undefined","undefined"]})
    );
    page.close();
}
