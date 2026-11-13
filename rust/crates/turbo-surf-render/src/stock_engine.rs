//! Default-profile stand-ins for secondary-realm module/code-generation services.
use super::{op2, v8, JsErrorBox};

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
