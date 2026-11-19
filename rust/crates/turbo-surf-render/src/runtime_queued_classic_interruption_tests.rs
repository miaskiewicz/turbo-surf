//! Genuine queued classic scripts settle their waiter and preserve healthy realms.
use super::*;

const REACHED: &str = "document.documentElement.setAttribute('data-anonymous',String(document.currentScript===null));document.querySelector('#state').textContent='reached';";

struct QueuedCase {
    child: v8::Global<v8::Context>,
    promise: v8::Global<v8::Promise>,
    page: PageSession,
}

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, key)
        .expect("authored own data property")
}

async fn queued_case(endless: bool) -> QueuedCase {
    let mut page = PageSession::open(
        "<body><p id=root>root</p><iframe id=child srcdoc='<body><p id=state>before</p>'></iframe><iframe id=peer srcdoc='<body><p>peer</p>'></iframe></body>",
        "https://queued-classic.test/", "", "", 2000,
    ).await.unwrap();
    page.eval(r#"
      globalThis.childHost=document.querySelector('#child');
      globalThis.childDocument=childHost.contentDocument;
      globalThis.rootDocument=document;
      globalThis.rootMarker={kind:'root'};globalThis.savedRootMarker=rootMarker;
      globalThis.peerHost=document.querySelector('#peer');globalThis.peerDocument=peerHost.contentDocument;
      peerHost.contentWindow.peerMarker={kind:'peer'};globalThis.savedPeerMarker=peerHost.contentWindow.peerMarker;
      globalThis.queuedOutcome={kind:'pending'};
      globalThis.onQueuedSuccess=()=>{queuedOutcome={kind:'fulfilled'}};
      globalThis.onQueuedFailure=error=>{queuedOutcome={kind:'rejected',name:error.name,message:error.message,ownRealm:error instanceof Error}};
    "#).await.unwrap();
    assert_eq!(page.read_engine("return [childDocument.querySelector('#state').textContent,childDocument.currentScript===null,rootDocument===document,peerDocument===peerHost.contentDocument]").unwrap(), "[\"before\",true,true,true]");
    let (child, promise) = {
        let _activation = page.rt.activation_key().enter();
        let root = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, root);
        let scope = &mut v8::ContextScope::new(scope, root);
        assert_eq!(scope.get_entered_or_microtask_context(), root);
        let global = root.global(scope);
        let document = own(scope, global, "childDocument")
            .to_object(scope)
            .unwrap();
        let child = document.get_creation_context(scope).unwrap();
        assert_ne!(child, root);
        assert!(child.get_slot::<FrameRetired>().is_none());
        let actual = child
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        assert!(actual.strict_equals(document.into()));
        let core = root.get_slot::<RuntimeCore>().unwrap();
        let core = crate::context_ownership::local(scope, &core.0);
        let ops = own(scope, core, "ops").to_object(scope).unwrap();
        let queue =
            v8::Local::<v8::Function>::try_from(own(scope, ops, "op_queue_frame_script")).unwrap();
        let fulfilled =
            v8::Local::<v8::Function>::try_from(own(scope, global, "onQueuedSuccess")).unwrap();
        let rejected =
            v8::Local::<v8::Function>::try_from(own(scope, global, "onQueuedFailure")).unwrap();
        let source = if endless {
            format!("{REACHED}for (;;) {{}}")
        } else {
            REACHED.to_owned()
        };
        let code = v8::String::new(scope, &source).unwrap();
        let url = v8::String::new(
            scope,
            "https://queued-classic.test/anonymous-host-script.js",
        )
        .unwrap();
        let response = v8::Integer::new_from_unsigned(scope, 0);
        // The admitted document has finished loading, so the production anonymous
        // source default is honest; its parser script inventory may be cleared.
        let index = v8::Integer::new_from_unsigned(scope, u32::MAX);
        let window = child.global(scope);
        let promise = async_owner::with_document(scope, root, |scope| {
            let receiver = v8::undefined(scope);
            let value = queue
                .call(
                    scope,
                    receiver.into(),
                    &[
                        window.into(),
                        code.into(),
                        url.into(),
                        response.into(),
                        index.into(),
                    ],
                )
                .expect("actual registered queue accepts the live same-origin child");
            let promise = v8::Local::<v8::Promise>::try_from(value).unwrap();
            assert_eq!(promise.state(), v8::PromiseState::Pending);
            promise
                .then2(scope, fulfilled, rejected)
                .expect("ordinary authored waiter handlers");
            v8::Global::new(scope, promise)
        })
        .expect("actual Root requester owns the queued operation");
        (v8::Global::new(scope, child), promise)
    };
    QueuedCase {
        page,
        child,
        promise,
    }
}

fn assert_native_outcome(case: &mut QueuedCase, interrupted: bool) {
    let _activation = case.page.rt.activation_key().enter();
    let root = private_infrastructure::context(&mut case.page.rt).unwrap();
    deno_core::scope!(scope, case.page.rt);
    let root = v8::Local::new(scope, root);
    let scope = &mut v8::ContextScope::new(scope, root);
    let child = v8::Local::new(scope, &case.child);
    assert_eq!(child.get_slot::<FrameRetired>().is_some(), interrupted);
    let promise = v8::Local::new(scope, &case.promise);
    if interrupted {
        assert_eq!(promise.state(), v8::PromiseState::Rejected);
        let error = promise.result(scope).to_object(scope).unwrap();
        let message = v8::String::new(scope, "message").unwrap();
        assert_eq!(
            error
                .get(scope, message.into())
                .unwrap()
                .to_rust_string_lossy(scope),
            "Classic script execution was interrupted"
        );
    } else {
        assert_eq!(promise.state(), v8::PromiseState::Fulfilled);
        assert!(promise.result(scope).is_undefined());
    }
}

fn assert_reached_and_isolated(page: &mut PageSession) {
    assert_eq!(page.read_engine("return [childDocument.querySelector('#state').textContent,childDocument.documentElement.getAttribute('data-anonymous'),childDocument.currentScript===null,document===rootDocument,rootMarker===savedRootMarker,peerHost.contentDocument===peerDocument,peerHost.contentWindow.peerMarker===savedPeerMarker,peerDocument.body.textContent,typeof Deno]").unwrap(), "[\"reached\",\"true\",true,true,true,true,true,\"peer\",\"undefined\"]");
}

#[tokio::test(flavor = "current_thread")]
async fn finite_queued_classic_settles_its_real_waiter_and_restores_anonymous_script_state() {
    let mut case = queued_case(false).await;
    assert_eq!(
        case.page
            .collect_engine("globalThis.__RESULT='collected'")
            .await
            .unwrap(),
        "collected"
    );
    assert_native_outcome(&mut case, false);
    assert_reached_and_isolated(&mut case.page);
    case.page.eval("").await.unwrap();
    assert_eq!(
        case.page.read_engine("return queuedOutcome").unwrap(),
        "{\"kind\":\"fulfilled\"}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn collector_watchdog_rejects_real_queued_classic_and_retires_only_its_child() {
    let mut case = queued_case(true).await;
    let error = case
        .page
        .collect_engine("globalThis.__RESULT='before interruption'")
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "Driver collection exceeded the session execution budget"
    );
    assert_native_outcome(&mut case, true);
    assert_reached_and_isolated(&mut case.page);
    // Remove the actual failed host before another hydrate turn can re-admit it.
    case.page
        .eval("childHost.remove();document.querySelector('#root').textContent='healthy'")
        .await
        .unwrap();
    assert_eq!(case.page.read_engine("return queuedOutcome").unwrap(), "{\"kind\":\"rejected\",\"name\":\"Error\",\"message\":\"Classic script execution was interrupted\",\"ownRealm\":true}");
    assert_eq!(
        case.page
            .collect_engine("globalThis.__RESULT='healthy collector'")
            .await
            .unwrap(),
        "healthy collector"
    );
    assert_eq!(case.page.read_engine("return [document.querySelector('#root').textContent,document===rootDocument,rootMarker===savedRootMarker,peerHost.contentDocument===peerDocument,peerHost.contentWindow.peerMarker===savedPeerMarker,document.querySelector('#child')===null]").unwrap(), "[\"healthy\",true,true,true,true,true]");
}
