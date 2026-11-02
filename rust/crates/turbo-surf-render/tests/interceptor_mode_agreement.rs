//! Consumer copy of turbo-dom-browser-env's accessor-vs-frame-read agreement check
//! (src/interceptor_mode.rs, `fork_accessor_agrees_with_frame_read`): in the V8 surf
//! links, the turbo-dom-v8 fork accessor the binding calls by default
//! (`PropertyCallbackArguments::should_throw_on_error`) reports the same mode as the
//! frame read the native-SDK overlay uses (`test_support::frame_read_should_throw`),
//! and both match the caller's language mode. Both engine configurations compile it
//! (turbo-surf#68): the setter/definer interceptors return `ReturnValue<()>` in the
//! turbo-dom-v8 fork and `ReturnValue<Boolean>` in the native SDK (`SetterResult`).
//! The native SDK links V8 without the fork's ShouldThrowOnError fix, so there the
//! accessor check is ignored (`native-sdk-frame-read`) and only the frame read is checked.
use std::cell::RefCell;
use std::sync::Once;
use turbo_dom_browser_env::test_support::frame_read_should_throw;

/// The setter/definer interceptor return slot of the linked V8 binding.
#[cfg(not(turbo_surf_native_proxy))]
type SetterResult<'r> = v8::ReturnValue<'r, ()>;
#[cfg(turbo_surf_native_proxy)]
type SetterResult<'r> = v8::ReturnValue<'r, v8::Boolean>;

thread_local! {
    /// (operation, frame read, V8 accessor) per intercepted call.
    static SEEN: RefCell<Vec<(&'static str, bool, bool)>> = const { RefCell::new(Vec::new()) };
}

fn initialize() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        v8::V8::initialize_platform(v8::new_default_platform(0, false).make_shared());
        v8::V8::initialize();
    });
}

fn record(
    operation: &'static str,
    scope: &mut v8::PinScope<'_, '_>,
    args: &v8::PropertyCallbackArguments<'_>,
) {
    let frame = frame_read_should_throw(scope, args);
    let accessor = args.should_throw_on_error();
    SEEN.with(|seen| seen.borrow_mut().push((operation, frame, accessor)));
}

fn named_setter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: v8::Local<'s, v8::Name>,
    _: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    _: SetterResult<'_>,
) -> v8::Intercepted {
    record("named-set", scope, &args);
    v8::Intercepted::kNo
}
fn named_definer<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: v8::Local<'s, v8::Name>,
    _: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments<'s>,
    _: SetterResult<'_>,
) -> v8::Intercepted {
    record("named-define", scope, &args);
    v8::Intercepted::kNo
}
fn named_deleter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    _: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    record("named-delete", scope, &args);
    v8::Intercepted::kNo
}
fn indexed_setter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: u32,
    _: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    _: SetterResult<'_>,
) -> v8::Intercepted {
    record("indexed-set", scope, &args);
    v8::Intercepted::kNo
}
fn indexed_definer<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: u32,
    _: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments<'s>,
    _: SetterResult<'_>,
) -> v8::Intercepted {
    record("indexed-define", scope, &args);
    v8::Intercepted::kNo
}
fn indexed_deleter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: u32,
    args: v8::PropertyCallbackArguments<'s>,
    _: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    record("indexed-delete", scope, &args);
    v8::Intercepted::kNo
}

/// Strict/sloppy/Reflect forms of every mutating interceptor, named and indexed,
/// with the mode Chrome's interceptors see for each. A definer that does not
/// intercept falls through to the setter, so each case only keeps the records of
/// its own operation.
/// (source, operation suffix, expected (operation, should throw) records).
type Case = (&'static str, &'static str, &'static [(&'static str, bool)]);

const CASES: [Case; 3] = [
    (
        "target.a = 1; (function () { 'use strict'; target.b = 1; })(); Reflect.set(target, 'c', 1);
         target[0] = 1; (function () { 'use strict'; target[1] = 1; })(); Reflect.set(target, 2, 1);",
        "-set",
        &[
            ("named-set", false),
            ("named-set", true),
            ("named-set", false),
            ("indexed-set", false),
            ("indexed-set", true),
            ("indexed-set", false),
        ],
    ),
    (
        "Object.defineProperty(target, 'a', {value: 1, configurable: true});
         Reflect.defineProperty(target, 'b', {value: 1, configurable: true});
         Object.defineProperty(target, 0, {value: 1, configurable: true});
         Reflect.defineProperty(target, 1, {value: 1, configurable: true});",
        "-define",
        &[
            ("named-define", true),
            ("named-define", false),
            ("indexed-define", true),
            ("indexed-define", false),
        ],
    ),
    (
        "delete target.a; (function () { 'use strict'; delete target.b; })(); Reflect.deleteProperty(target, 'c');
         delete target[0]; (function () { 'use strict'; delete target[1]; })(); Reflect.deleteProperty(target, 2);",
        "-delete",
        &[
            ("named-delete", false),
            ("named-delete", true),
            ("named-delete", false),
            ("indexed-delete", false),
            ("indexed-delete", true),
            ("indexed-delete", false),
        ],
    ),
];

fn observed(source: &str) -> Vec<(&'static str, bool, bool)> {
    initialize();
    SEEN.with(|seen| seen.borrow_mut().clear());
    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let template = v8::ObjectTemplate::new(scope);
    template.set_named_property_handler(
        v8::NamedPropertyHandlerConfiguration::new()
            .setter(named_setter)
            .definer(named_definer)
            .deleter(named_deleter),
    );
    template.set_indexed_property_handler(
        v8::IndexedPropertyHandlerConfiguration::new()
            .setter(indexed_setter)
            .definer(indexed_definer)
            .deleter(indexed_deleter),
    );
    let object = template.new_instance(scope).unwrap();
    let key = v8::String::new(scope, "target").unwrap();
    context.global(scope).set(scope, key.into(), object.into());
    let code = v8::String::new(scope, source).unwrap();
    v8::Script::compile(scope, code, None)
        .unwrap()
        .run(scope)
        .unwrap();
    SEEN.with(|seen| seen.borrow().clone())
}

#[test]
fn frame_read_matches_the_callers_language_mode() {
    for (source, suffix, expected) in CASES {
        let frame: Vec<_> = observed(source)
            .into_iter()
            .filter(|(operation, _, _)| operation.ends_with(suffix))
            .map(|(operation, frame, _)| (operation, frame))
            .collect();
        assert_eq!(frame, expected, "{suffix}");
    }
}

#[test]
#[cfg_attr(
    feature = "native-sdk-frame-read",
    ignore = "the linked V8 lacks the fork's ShouldThrowOnError fix (native-sdk-frame-read)"
)]
fn fork_accessor_agrees_with_frame_read() {
    for (source, _, _) in CASES {
        for (operation, frame, accessor) in observed(source) {
            assert_eq!(accessor, frame, "{operation}");
        }
    }
}
