//! Surf's native error-class registration must preserve canonical DOMExceptions.
use super::*;

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, name).unwrap()
}

async fn security_page(opaque_second: bool) -> PageSession {
    let sandbox = if opaque_second {
        "sandbox=allow-scripts"
    } else {
        ""
    };
    let html = format!("<body><p id=stable>root</p><iframe id=first srcdoc='<body>first'></iframe><iframe id=second {sandbox} srcdoc='<body>second'></iframe></body>");
    let mut page = PageSession::open(&html, "https://document-boundary.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval("globalThis.firstHost=document.querySelector('#first');globalThis.secondHost=document.querySelector('#second')").await.unwrap();
    page
}

#[tokio::test(flavor = "current_thread")]
async fn native_security_error_builder_returns_the_captured_canonical_exception() {
    let mut page = security_page(true).await;
    {
        let _activation = page.rt.activation_key().enter();
        let root = private_infrastructure::context(&mut page.rt).unwrap();
        let contexts = page
            .rt
            .op_state()
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .clone();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, root);
        let scope = &mut v8::ContextScope::new(scope, root);
        assert_eq!(scope.get_entered_or_microtask_context(), root);
        let core = root.get_slot::<RuntimeCore>().unwrap();
        let core = crate::context_ownership::local(scope, &core.0);
        let infrastructure = core.get_creation_context(scope).unwrap();
        // E's globals are private infrastructure, not a public constructor registry.
        // An existing builder captures the same canonical constructor as SecurityError.
        let builder =
            v8::Local::<v8::Function>::try_from(own(scope, core, "buildCustomError")).unwrap();
        let class = v8::String::new(scope, "DOMExceptionSyntaxError").unwrap();
        let message = v8::String::new(scope, "canonical builder witness").unwrap();
        let receiver = v8::undefined(scope);
        let baseline = builder
            .call(scope, receiver.into(), &[class.into(), message.into()])
            .unwrap()
            .to_object(scope)
            .unwrap();
        assert_eq!(baseline.get_creation_context(scope), Some(infrastructure));
        let prototype = baseline.get_prototype(scope).unwrap();
        for (key, expected) in [
            ("name", "SyntaxError"),
            ("message", "canonical builder witness"),
        ] {
            let key = v8::String::new(scope, key).unwrap();
            assert_eq!(
                baseline
                    .get(scope, key.into())
                    .unwrap()
                    .to_rust_string_lossy(scope),
                expected
            );
        }
        let key = v8::String::new(scope, "code").unwrap();
        assert_eq!(
            baseline.get(scope, key.into()).unwrap().uint32_value(scope),
            Some(12)
        );
        let ops = own(scope, core, "ops").to_object(scope).unwrap();
        let function =
            v8::Local::<v8::Function>::try_from(own(scope, ops, "op_frame_host_helper")).unwrap();
        let root_global = root.global(scope);
        let first_host = own(scope, root_global, "firstHost");
        let foreign_host = own(scope, root_global, "secondHost");
        let mut foreign = None;
        for candidate in &contexts {
            let context = v8::Local::new(scope, candidate);
            if context.get_slot::<FrameRetired>().is_none()
                && context.get_slot::<FrameHost>().is_some_and(|host| {
                    host.0
                        .to_local(scope)
                        .is_some_and(|host| host.strict_equals(foreign_host))
                })
            {
                foreign = Some(context);
                break;
            }
        }
        let foreign = foreign.expect("actual opaque child selected by real native host");
        assert!(!root
            .get_security_token(scope)
            .strict_equals(foreign.get_security_token(scope)));
        let document = root
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let foreign_document = foreign
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let name = v8::String::new(scope, "frameRecordFor").unwrap();
        let arguments = v8::Array::new_with_elements(scope, &[first_host]);
        let receiver = v8::undefined(scope);
        let record = function
            .call(
                scope,
                receiver.into(),
                &[document.into(), name.into(), arguments.into()],
            )
            .unwrap();
        let record = record.to_object(scope).unwrap();
        assert!(own(scope, record, "owner").strict_equals(root_global.into()));
        let exception = {
            v8::tc_scope!(let scope, scope);
            let receiver = v8::undefined(scope);
            assert!(function
                .call(
                    scope,
                    receiver.into(),
                    &[foreign_document.into(), name.into(), arguments.into()]
                )
                .is_none());
            assert!(!scope.has_terminated());
            let exception = scope.exception().expect("actual native security rejection");
            v8::Global::new(scope, exception)
        };
        assert!(foreign.get_slot::<FrameRetired>().is_none());
        let after = foreign
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        assert!(after.strict_equals(foreign_document.into()));
        let exception = v8::Local::new(scope, exception);
        let exception = v8::Local::<v8::Object>::try_from(exception)
            .expect("native rejection must be a DOMException, never undefined");
        assert_eq!(exception.get_creation_context(scope), Some(infrastructure));
        assert!(exception
            .get_prototype(scope)
            .unwrap()
            .strict_equals(prototype));
        for (key, expected) in [
            ("name", "SecurityError"),
            ("message", "Frame owner is cross-origin"),
        ] {
            let key = v8::String::new(scope, key).unwrap();
            assert_eq!(
                exception
                    .get(scope, key.into())
                    .unwrap()
                    .to_rust_string_lossy(scope),
                expected
            );
        }
        let key = v8::String::new(scope, "code").unwrap();
        assert_eq!(
            exception
                .get(scope, key.into())
                .unwrap()
                .uint32_value(scope),
            Some(18)
        );
    }
    assert_eq!(page.read_engine("return [document.querySelector('#stable').textContent,firstHost.contentDocument.body.textContent,typeof Deno]").unwrap(), "[\"root\",\"first\",\"undefined\"]");
}

#[tokio::test(flavor = "current_thread")]
async fn public_security_errors_preserve_each_live_realm_after_guest_constructor_poisoning() {
    let mut page = security_page(false).await;
    let result = page.eval_json_expression(r#"(() => {
      function exercise(window) {
        const Exception = window.DOMException;
        const prototype = Exception.prototype;
        const getPrototypeOf = window.Object.getPrototypeOf;
        const originalString = window.String;
        const beforeURL = window.location.href;
        const beforeLength = window.history.length;
        let poisonCalls = 0;
        window.DOMException = function(){ poisonCalls++; throw new Error('poisoned DOMException'); };
        window.String = function(){ poisonCalls++; throw new Error('poisoned String'); };
        let caught;
        try { window.history.pushState({forbidden:true}, '', 'https://other-document-boundary.test/forbidden'); }
        catch(error) { caught = error; }
        const result = {
          defined: caught !== undefined,
          canonical: caught instanceof Exception,
          prototype: caught !== undefined && getPrototypeOf(caught) === prototype,
          name: caught?.name,
          code: caught?.code,
          message: typeof caught?.message === 'string' && caught.message.length > 0,
          urlUnchanged: window.location.href === beforeURL,
          lengthUnchanged: window.history.length === beforeLength,
          poisonCalls,
        };
        window.DOMException = Exception;
        window.String = originalString;
        window.history.replaceState({healthy:true}, '');
        result.validStateOnly = window.history.state.healthy === true && window.location.href === beforeURL;
        return result;
      }
      return [exercise(globalThis), exercise(firstHost.contentWindow), exercise(secondHost.contentWindow)];
    })()"#).await.unwrap();
    let expected = serde_json::json!({
        "defined":true,"canonical":true,"prototype":true,"name":"SecurityError","code":18,
        "message":true,"urlUnchanged":true,"lengthUnchanged":true,"poisonCalls":0,"validStateOnly":true,
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!([expected, expected, expected])
    );
    page.eval("document.querySelector('#first').remove();const next=document.createElement('iframe');next.id='replacement';next.srcdoc='<body>replacement';document.body.append(next)").await.unwrap();
    assert_eq!(page.read_engine("return [document.querySelector('#stable').textContent,document.querySelector('#replacement').contentDocument.body.textContent,typeof Deno]").unwrap(), "[\"root\",\"replacement\",\"undefined\"]");
}
