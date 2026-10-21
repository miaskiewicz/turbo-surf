//! Native SDK primitive checks, run only in the isolated exact-engine build.
//! Passing these is required before the runtime can adopt a stable WindowProxy.
use deno_core::{JsRuntime, RuntimeOptions, v8};
use std::rc::Rc;

struct WindowDocument(v8::Global<v8::Value>);
struct WindowProxy(v8::Global<v8::Object>);

fn window_document_get(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let context = scope.get_current_context();
    let Some(proxy) = context.get_slot::<WindowProxy>() else {
        return;
    };
    let proxy = v8::Local::new(scope, &proxy.0);
    let Some(active) = proxy.get_creation_context(scope) else {
        return;
    };
    if !context.get_security_token(scope).strict_equals(active.get_security_token(scope)) { deny(scope);return; }
    if let Some(document) = active.get_slot::<WindowDocument>() {
        result.set(v8::Local::new(scope, &document.0));
    }
}

fn window_document_set(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    let context = scope.get_current_context();
    context.set_slot(Rc::new(WindowDocument(v8::Global::new(scope, args.get(0)))));
}

fn access_allowed<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    accessing: v8::Local<'s, v8::Context>,
    accessed: v8::Local<'s, v8::Object>,
    _data: v8::Local<'s, v8::Value>,
) -> bool {
    accessed.get_creation_context(scope).is_some_and(|owner| {
        owner
            .get_security_token(scope)
            .strict_equals(accessing.get_security_token(scope))
    })
}

fn deny<'s>(scope: &mut v8::PinScope<'s, '_>) {
    let message = v8::String::new(scope, "Blocked cross-origin window access").unwrap();
    let exception = v8::Exception::error(scope, message);
    let name = v8::String::new(scope, "name").unwrap();
    let security_error = v8::String::new(scope, "SecurityError").unwrap();
    let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
    object.set(scope, name.into(), security_error.into());
    scope.throw_exception(exception);
}

fn cross_origin_get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    match name.to_rust_string_lossy(scope).as_str() {
        "self" | "window" => result.set(args.holder().into()),
        "closed" => result.set_bool(false),
        _ => deny(scope),
    }
    v8::Intercepted::kYes
}

fn cross_origin_index<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _index: u32,
    _args: v8::PropertyCallbackArguments<'s>,
    _result: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    deny(scope);
    v8::Intercepted::kYes
}

fn eval<'s>(scope: &mut v8::PinScope<'s, '_>, source: &str) -> v8::Local<'s, v8::Value> {
    let code = v8::String::new(scope, source).unwrap();
    v8::tc_scope!(let scope, scope);
    let result = v8::Script::compile(scope, code, None).and_then(|script| script.run(scope));
    match result {
        Some(value) => value,
        None => {
            let error = scope
                .exception()
                .map(|value| value.to_rust_string_lossy(scope))
                .unwrap_or_default();
            panic!("{source}: {error}");
        }
    }
}

async fn check_replacement(cross_origin: bool) {
    let mut runtime = JsRuntime::new(RuntimeOptions::default());
    deno_core::scope!(scope, runtime);
    let parent = scope.get_current_context();
    let template = v8::ObjectTemplate::new(scope);
    template.set_access_check_callback_and_handler(
        access_allowed,
        v8::NamedPropertyHandlerConfiguration::new().getter(cross_origin_get),
        v8::IndexedPropertyHandlerConfiguration::new().getter(cross_origin_index),
        None,
    );
    let document_key = v8::String::new(scope, "document").unwrap();
    let getter = v8::FunctionTemplate::new(scope, window_document_get);
    let setter = v8::FunctionTemplate::new(scope, window_document_set);
    template.set_accessor_property(
        document_key.into(),
        Some(getter),
        Some(setter),
        v8::PropertyAttribute::DONT_DELETE,
    );
    let first = v8::Context::new(
        scope,
        v8::ContextOptions {
            global_template: Some(template),
            ..Default::default()
        },
    );
    first.set_security_token(parent.get_security_token(scope));
    let proxy = first.global(scope);
    first.set_slot(Rc::new(WindowProxy(v8::Global::new(scope, proxy))));
    let document = {
        let scope = &mut v8::ContextScope::new(scope, first);
        eval(scope, "globalThis.document={marker:'old'};document")
    };
    let function = {
        let scope = &mut v8::ContextScope::new(scope, first);
        eval(scope, "Function")
    };
    let (compiled, global_compiled) = {
        let scope=&mut v8::ContextScope::new(scope,first);
        (eval(scope,"Function('return document')"),eval(scope,"Function('return globalThis.document')"))
    };
    for (name, value) in [
        ("child", proxy.into()),
        ("oldDocument", document),
        ("oldFunction", function),
        ("oldCode", compiled),
        ("oldGlobalCode", global_compiled),
    ] {
        let name = v8::String::new(scope, name).unwrap();
        parent.global(scope).set(scope, name.into(), value);
    }
    assert!(eval(scope, "child.document===oldDocument").is_true());

    first.detach_global();
    let replacement = v8::Context::new(
        scope,
        v8::ContextOptions {
            global_template: Some(template),
            global_object: Some(proxy.into()),
            ..Default::default()
        },
    );
    let token = if cross_origin {
        v8::String::new(scope, "https://other.invalid")
            .unwrap()
            .into()
    } else {
        parent.get_security_token(scope)
    };
    replacement.set_security_token(token);
    let replacement_proxy = replacement.global(scope);
    replacement.set_slot(Rc::new(WindowProxy(v8::Global::new(
        scope,
        replacement_proxy,
    ))));
    // Detached functions retain their lexical context. A cross-origin commit
    // revokes access to that realm while old ordinary Document objects survive.
    if cross_origin { first.use_default_security_token(); }
    else { first.set_security_token(token); }
    assert!(proxy.strict_equals(replacement_proxy.into()));
    assert_eq!(proxy.get_creation_context(scope), Some(replacement));
    {
        let scope = &mut v8::ContextScope::new(scope, replacement);
        assert!(
            eval(
                scope,
                "globalThis.document={marker:'new'};document.marker==='new'"
            )
            .is_true()
        );
    }
    assert!(eval(scope, "oldDocument.marker==='old'").is_true());
    if cross_origin {
        assert!(
            eval(
                scope,
                "try{oldFunction('return document');false}catch(e){e.name==='TypeError'}"
            )
            .is_true(),
            "Chrome rejects compiling through the formerly same-origin Function constructor"
        );
        for code in ["oldCode()","oldGlobalCode()"] {
            assert!(eval(scope,&format!("try{{{code};false}}catch(e){{e.name==='SecurityError'}}")).is_true(),"retained compiled code must not reach a new origin");
        }
        assert!(
            eval(scope, "Object.getPrototypeOf(child)===null").is_true(),
            "Chrome exposes a null cross-origin prototype"
        );
        assert!(
            eval(
                scope,
                "child===child.window && child===child.self && child.closed===false"
            )
            .is_true()
        );
        for expression in [
            "child.document",
            "child.Function",
            "child[0]",
            "child.document={}",
            "Object.defineProperty(child,'document',{value:{}})",
            "Object.setPrototypeOf(child,{})",
            "Object.getOwnPropertyDescriptor(child,'document')",
            "'document' in child",
            "delete child.document",
        ] {
            assert!(
                eval(scope, &format!("try{{{expression};false}}catch(e){{true}}")).is_true(),
                "cross-origin operation was allowed: {expression}"
            );
        }
    } else {
        assert!(
            eval(scope, "typeof oldFunction('return document')==='function'").is_true(),
            "Chrome permits retained same-origin Function construction"
        );
        assert!(
            eval(scope, "oldFunction('return document')()===child.document").is_true(),
            "the retained Function must resolve document in the replacement global"
        );
        assert!(
            eval(
                scope,
                "oldFunction('return globalThis.document')()===child.document"
            )
            .is_true(),
            "the retained Function must reach the replacement through the stable global proxy"
        );
        assert!(
            eval(
                scope,
                "child.document.marker==='new' && child.document!==oldDocument"
            )
            .is_true()
        );
    }
}

#[tokio::test]
async fn same_origin_navigation_keeps_proxy_identity_and_separate_document_ownership() {
    check_replacement(false).await;
}

#[tokio::test]
async fn cross_origin_navigation_uses_native_access_checks_on_the_same_proxy() {
    check_replacement(true).await;
}
