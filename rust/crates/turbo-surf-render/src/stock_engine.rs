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
