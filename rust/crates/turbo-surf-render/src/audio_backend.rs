//! Private native offline DSP operations and per-realm Web Audio interfaces.
use deno_core::{op2, v8, Extension};
use deno_error::JsErrorBox;

#[op2]
#[serde]
fn op_audio_render(
    #[serde] graph: crate::audio_dsp::Graph,
) -> Result<crate::audio_dsp::Rendered, JsErrorBox> {
    crate::audio_dsp::render(graph).map_err(JsErrorBox::generic)
}

#[op2]
fn op_audio_param_value(
    initial: f32,
    #[serde] automation: crate::audio_timeline::Automation,
    time: f64,
    rate: f64,
) -> Result<f32, JsErrorBox> {
    crate::audio_timeline::value_at(initial, automation, time, rate).map_err(JsErrorBox::generic)
}

#[op2]
#[serde]
fn op_audio_frequency_response(
    #[string] kind: &str,
    rate: f32,
    #[serde] params: Vec<f32>,
    #[serde] frequencies: Vec<f32>,
) -> Result<(Vec<f32>, Vec<f32>), JsErrorBox> {
    crate::audio_effects::Effect::frequency_response(kind, rate, &params, &frequencies)
        .map_err(JsErrorBox::generic)
}

#[op2]
fn op_audio_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    prototype: v8::Local<'s, v8::Object>,
    event_target: bool,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    // An opaque V8 host object prevents structuredClone from copying or forging
    // private audio state. The sole record is guest data behind a private key.
    let template = v8::ObjectTemplate::new(scope);
    template.set_internal_field_count(1);
    let object = template
        .new_instance(scope)
        .ok_or_else(|| JsErrorBox::generic("Offline audio object allocation interrupted"))?;
    if object.set_prototype(scope, prototype.into()) != Some(true) {
        return Err(JsErrorBox::generic(
            "Offline audio object prototype installation interrupted",
        ));
    }
    if event_target && !crate::browser_env::register_native_event_target(scope, object) {
        return Err(JsErrorBox::generic(
            "Offline audio event-target registration interrupted",
        ));
    }
    Ok(object)
}

deno_core::extension!(
    turbo_audio,
    ops = [
        op_audio_render,
        op_audio_object,
        op_audio_param_value,
        op_audio_frequency_response
    ]
);
pub(crate) fn extension() -> Extension {
    turbo_audio::init()
}

pub(crate) fn install<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    core: v8::Local<'s, v8::Object>,
) -> Result<(), String> {
    let namespace =
        crate::browser_env::engine_namespace(scope).ok_or("native audio namespace missing")?;
    let key = v8::String::new(scope, "makeNativeFunction").unwrap();
    let factory = namespace
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or("native audio function factory missing")?;
    v8::tc_scope!(let scope,scope);
    let source = v8::String::new(scope, include_str!("audio_backend.js")).unwrap();
    let initializer = v8::Script::compile(scope, source, None)
        .and_then(|script| script.run(scope))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
    if let Some(initializer) = initializer {
        let receiver = v8::undefined(scope);
        if initializer
            .call(scope, receiver.into(), &[core.into(), factory.into()])
            .is_some()
        {
            return Ok(());
        }
    }
    Err(scope
        .exception()
        .map(|error| error.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "Native offline audio installation failed".into()))
}
