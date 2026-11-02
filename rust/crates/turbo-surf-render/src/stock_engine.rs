//! Stock-SDK stand-ins for the native-engine modules (`window_proxy.rs`,
//! `frame_modules.rs`, `code_generation.rs`). Without the source-built SDK a frame is
//! a plain V8 context, Location stays in the binding's JS, child-frame modules are
//! unsupported and eval admission is the context's code-generation flag.
use super::{op2, v8, JsErrorBox};

pub(super) mod window_proxy {
    use super::v8;

    pub(in crate::runtime) fn synchronize_origins(_: &mut v8::PinScope, _: v8::Local<v8::Context>) {
    }
    pub(in crate::runtime) fn install_document(_: &mut v8::PinScope) {}
    pub(in crate::runtime) fn capture_exception(_: &mut v8::PinScope) {}
    pub(in crate::runtime) fn install_location(_: &mut v8::PinScope, _: &str) {}
    pub(in crate::runtime) fn location_url(_: v8::Local<v8::Context>) -> Option<String> {
        None
    }
    fn illegal_window_constructor(
        scope: &mut v8::PinScope,
        args: v8::FunctionCallbackArguments,
        _result: v8::ReturnValue,
    ) {
        let text = if args.new_target().is_undefined() {
            "Illegal constructor"
        } else {
            "Failed to construct 'Window': Illegal constructor"
        };
        let message = v8::String::new(scope, text).unwrap();
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
    }

    /// The main realm's global is a `Window` instance whose [[SetPrototypeOf]] refuses any
    /// other prototype, as in Chrome (an immutable-prototype exotic object); the binding
    /// completes the chain above `Window.prototype` (turbo-dom#223).
    pub(in crate::runtime) fn template<'s>(
        scope: &mut v8::PinScope<'s, '_, ()>,
        _template: v8::Local<'s, v8::ObjectTemplate>,
    ) -> v8::Local<'s, v8::ObjectTemplate> {
        let constructor = v8::FunctionTemplate::new(scope, illegal_window_constructor);
        let name = v8::String::new(scope, "Window").unwrap();
        constructor.set_class_name(name);
        constructor.read_only_prototype();
        let template = constructor.instance_template(scope);
        template.set_with_attr(
            name.into(),
            constructor.into(),
            v8::PropertyAttribute::DONT_ENUM,
        );
        template.set_immutable_proto();
        template
    }

    pub(in crate::runtime) fn context<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        _previous: Option<v8::Local<'s, v8::Context>>,
    ) -> v8::Local<'s, v8::Context> {
        v8::Context::new(scope, Default::default())
    }
}

pub(super) mod frame_modules {
    use super::{op2, v8, JsErrorBox};

    pub(in crate::runtime) fn register_frame_modules(
        _: &mut v8::PinScope,
        _: &str,
    ) -> Result<(), JsErrorBox> {
        Ok(())
    }

    /// The frame runner reports this limit; the frame's classic scripts still run.
    #[op2(fast)]
    pub(in crate::runtime) fn op_frame_module(
        _window: v8::Local<v8::Object>,
        _index: u32,
    ) -> Result<(), JsErrorBox> {
        Err(JsErrorBox::generic(
            "Child-frame ES modules require the exact native SDK",
        ))
    }
}

pub(super) mod code_generation {
    use super::v8;

    pub(in crate::runtime) fn install_code_generation_authorization(
        _: &mut v8::PinScope,
    ) -> Result<(), String> {
        Ok(())
    }
}
