use deno_core::v8;
use std::{cell::Cell, rc::Rc};
struct Allowed(Cell<bool>);
fn authorize(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let context = scope.get_current_context();
    let allowed = context.get_slot::<Allowed>().unwrap();
    if args.get(0).is_string() && args.get(0).to_rust_string_lossy(scope) == "deny-native" {
        let message = v8::String::new(scope, "Native checked denial").unwrap();
        let detail = v8::Array::new(scope, 1);
        detail.set_index(scope, 0, message.into());
        result.set(detail.into());
        return;
    }
    if let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) {
        if !object.is_proxy() {
            let name = v8::String::new(scope, "test.private-typed-source").unwrap();
            let key = v8::Private::for_api(scope, Some(name));
            if let Some(source) = object
                .get_private(scope, key)
                .filter(|value| value.is_string())
            {
                result.set(source);
                return;
            }
        }
    }
    result.set_bool(!args.get(0).is_string() || allowed.0.get());
}
#[test]
fn private_code_generation_preserves_lexical_context_and_cannot_be_forged_with_public_symbols() {
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope,isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    context.set_slot(Rc::new(Allowed(Cell::new(true))));
    let callback = v8::Function::new(scope, authorize).unwrap();
    assert!(context.set_code_generation_callback(callback));
    let child = v8::Context::new(scope, Default::default());
    assert!(!child.set_code_generation_callback(callback));
    let key = v8::String::new(scope, "testCallback").unwrap();
    assert!(context
        .global(scope)
        .set(scope, key.into(), callback.into())
        .unwrap());
    for source in ["testCallback.bind(undefined)", "new Proxy(testCallback,{})"] {
        let source = v8::String::new(scope, source).unwrap();
        let script = v8::Script::compile(scope, source, None).unwrap();
        let value = script.run(scope).unwrap();
        let function = v8::Local::<v8::Function>::try_from(value).unwrap();
        assert!(!context.set_code_generation_callback(function));
    }
    assert!(context.global(scope).delete(scope, key.into()).unwrap());
    context.set_allow_generation_from_strings(false);
    let source=v8::String::new(scope,"(()=>{const lexical=7;const object={toString(){throw Error('coerced')}};return eval('lexical+1')===8 && eval(object)===object && new Function('return typeof lexical')()==='undefined' && Object.getOwnPropertySymbols(globalThis).every(symbol=>String(symbol)!=='Symbol(turbo-surf.code-generation-callback)');})()").unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    assert!(script.run(scope).unwrap().is_true());
    let typed = v8::Object::new(scope);
    let name = v8::String::new(scope, "test.private-typed-source").unwrap();
    let private = v8::Private::for_api(scope, Some(name));
    let source = v8::String::new(scope, "lexical + 2").unwrap();
    typed.set_private(scope, private, source.into()).unwrap();
    let key = v8::String::new(scope, "typed").unwrap();
    context
        .global(scope)
        .set(scope, key.into(), typed.into())
        .unwrap();
    let source = v8::String::new(scope,"(()=>{const lexical=10;Object.defineProperty(typed,'toString',{value(){throw Error('poison')}});const object={toString(){throw Error('coerced')}};let denied=false;try{eval('deny-native')}catch(error){denied=error instanceof EvalError&&error.message==='Native checked denial'}return denied&&eval(typed)===12&&eval(object)===object&&eval(new Proxy(typed,{}))!==12})()").unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    assert!(script.run(scope).unwrap().is_true());
    context.get_slot::<Allowed>().unwrap().0.set(false);
    let source=v8::String::new(scope,"(()=>{globalThis[Symbol.for('turbo-surf.code-generation-callback')]=()=>true;const object={};let errors=0;for(const run of [()=>eval('1'),()=>new Function('return 1')()])try{run()}catch(error){if(error.name==='EvalError')errors++}return errors===2&&eval(object)===object;})()").unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    assert!(script.run(scope).unwrap().is_true());
}
