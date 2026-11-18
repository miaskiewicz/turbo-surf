//! Native embedding contracts over genuine registered fetch ops and live page state.
use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, name)
        .expect("fixture own data property")
}

fn registered<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    name: &str,
) -> v8::Local<'s, v8::Function> {
    assert!(matches!(name, "op_fetch_prepare" | "op_fetch_release"));
    let core = context
        .get_slot::<RuntimeCore>()
        .expect("actual private runtime core");
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = own(scope, core, "ops");
    let ops = v8::Local::<v8::Object>::try_from(ops).expect("registered ops object");
    let function = own(scope, ops, name);
    v8::Local::<v8::Function>::try_from(function).expect("genuine registered native function")
}

fn prepare<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    function: v8::Local<'s, v8::Function>,
    document: v8::Local<'s, v8::Object>,
) -> Result<u32, String> {
    v8::tc_scope!(let scope, scope);
    let receiver = v8::undefined(scope);
    if let Some(value) = function.call(scope, receiver.into(), &[document.into()]) {
        assert!(
            value.is_uint32(),
            "registered prepare returns a numeric token"
        );
        return Ok(value.uint32_value(scope).unwrap());
    }
    assert!(
        !scope.has_terminated(),
        "fixture must not hit execution termination"
    );
    let exception = scope.exception().expect("native rejection exception");
    assert!(exception.is_native_error());
    let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
    assert_eq!(
        object.get_constructor_name().to_rust_string_lossy(scope),
        "TypeError"
    );
    let message = own(scope, object, "message");
    assert!(message.is_string());
    Err(message.to_rust_string_lossy(scope))
}

fn release<'s>(scope: &mut v8::PinScope<'s, '_>, function: v8::Local<'s, v8::Function>, id: u32) {
    v8::tc_scope!(let scope, scope);
    let receiver = v8::undefined(scope);
    let id = v8::Integer::new_from_unsigned(scope, id);
    let result = function.call(scope, receiver.into(), &[id.into()]);
    assert!(result.is_some_and(|value| value.is_null()));
    assert!(!scope.has_caught());
}

async fn fixture() -> (PageSession, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = requests.clone();
    let server = tokio::spawn(async move {
        let (mut socket, _) =
            tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
        observed.fetch_add(1, Ordering::SeqCst);
        let mut bytes = Vec::new();
        while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            let mut chunk = [0; 1024];
            let count = socket.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            assert!(bytes.len() <= 16 * 1024);
        }
        assert!(bytes.starts_with(b"GET /healthy HTTP/1.1\r\n"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nhealthy")
            .await
            .unwrap();
    });
    let page = PageSession::open("<body><p id=stable>owner</p>", &base, "", "", 5000)
        .await
        .unwrap();
    (page, server, requests)
}

async fn healthy(
    page: &mut PageSession,
    server: tokio::task::JoinHandle<()>,
    requests: Arc<AtomicUsize>,
) {
    let result = page.eval_json_expression("fetch('/healthy').then(response=>response.text()).then(body=>({body,stable:document.querySelector('#stable').textContent,privateCore:typeof Deno}))").await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"body":"healthy","stable":"owner","privateCore":"undefined"})
    );
    server.await.unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn registered_fetch_prepare_rejects_lookalikes_without_consuming_request_identity() {
    let (mut page, server, requests) = fixture().await;
    page.eval("globalThis.fetchTrapCalls=0;globalThis.fetchFake=Object.create(Document.prototype);globalThis.fetchProxy=new Proxy(document,{get(){fetchTrapCalls++;throw new Error('proxy get')},getPrototypeOf(){fetchTrapCalls++;throw new Error('proxy prototype')}});").await.unwrap();
    let log = page.network_event_log();
    let target = page.replaced_root_target().unwrap().to_string();
    {
        let _activation = page.rt.activation_key().enter();
        let context = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, page.rt);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let document = context
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let prepare_op = registered(scope, context, "op_fetch_prepare");
        let release_op = registered(scope, context, "op_fetch_release");
        let first = prepare(scope, prepare_op, document).unwrap();
        release(scope, release_op, first);
        let before = log.target_network_state(&target);
        let rows = log.revision();
        for name in ["fetchFake", "fetchProxy"] {
            let global = context.global(scope);
            let value = own(scope, global, name);
            let value = v8::Local::<v8::Object>::try_from(value).unwrap();
            assert_eq!(
                prepare(scope, prepare_op, value).unwrap_err(),
                "Fetch document unavailable"
            );
            assert_eq!(log.target_network_state(&target), before);
            assert_eq!(log.revision(), rows);
        }
        let next = prepare(scope, prepare_op, document).unwrap();
        assert_eq!(next, first + 1);
        release(scope, release_op, next);
        assert_eq!(log.target_network_state(&target)["pendingRequests"], 0);
    }
    assert_eq!(
        page.eval_json_expression("fetchTrapCalls").await.unwrap(),
        "0"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    healthy(&mut page, server, requests).await;
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn registered_fetch_prepare_rejects_retired_document_and_accepts_real_replacement() {
    let (mut page, server, requests) = fixture().await;
    page.eval("const frame=document.createElement('iframe');document.body.append(frame);globalThis.fetchRetired=frame.contentDocument;frame.remove();const replacement=document.createElement('iframe');document.body.append(replacement);globalThis.fetchReplacement=replacement.contentDocument;").await.unwrap();
    let log = page.network_event_log();
    let target = page.replaced_root_target().unwrap().to_string();
    {
        let _activation = page.rt.activation_key().enter();
        let context = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, page.rt);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let document = context
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let prepare_op = registered(scope, context, "op_fetch_prepare");
        let release_op = registered(scope, context, "op_fetch_release");
        let first = prepare(scope, prepare_op, document).unwrap();
        release(scope, release_op, first);
        let before = log.target_network_state(&target);
        let rows = log.revision();
        let global = context.global(scope);
        let retired = own(scope, global, "fetchRetired");
        let retired = v8::Local::<v8::Object>::try_from(retired).unwrap();
        assert_eq!(
            prepare(scope, prepare_op, retired).unwrap_err(),
            "Fetch document has been retired"
        );
        assert_eq!(log.target_network_state(&target), before);
        assert_eq!(log.revision(), rows);
        let replacement = own(scope, global, "fetchReplacement");
        let replacement = v8::Local::<v8::Object>::try_from(replacement).unwrap();
        let next = prepare(scope, prepare_op, replacement).unwrap();
        assert_eq!(next, first + 1);
        release(scope, release_op, next);
        assert_eq!(log.target_network_state(&target)["pendingRequests"], 0);
    }
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    healthy(&mut page, server, requests).await;
    page.close();
}
