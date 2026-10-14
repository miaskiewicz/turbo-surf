//! Native opaque rectangle objects with per-realm private interface records.
use deno_core::{op2, v8, Extension};
use deno_error::JsErrorBox;

#[op2]
fn op_geometry_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    prototype: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    let template = v8::ObjectTemplate::new(scope);
    template.set_internal_field_count(1);
    let object = template
        .new_instance(scope)
        .ok_or_else(|| JsErrorBox::generic("Geometry object allocation interrupted"))?;
    if object.set_prototype(scope, prototype.into()) != Some(true) {
        return Err(JsErrorBox::generic(
            "Geometry prototype installation interrupted",
        ));
    }
    Ok(object)
}
#[op2(fast)]
fn op_live_geometry_brand<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Value>,
    kind: u32,
) -> bool {
    crate::browser_env::valid_native_geometry_receiver(scope, receiver, kind)
}
deno_core::extension!(
    turbo_geometry,
    ops = [
        op_geometry_object,
        op_live_geometry_brand,
        crate::live_layout_backend::op_live_element_geometry,
        crate::live_layout_backend::op_live_element_scroll,
        crate::live_layout_backend::op_live_elements_at_point,
        crate::live_layout_backend::op_live_element_offset
    ]
);
pub(crate) fn extension() -> Extension {
    turbo_geometry::init()
}

fn factory_key<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, "turbo-surf.geometry.factory")?;
    Some(v8::Private::for_api(scope, Some(name)))
}
pub(crate) fn install<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    core: v8::Local<'s, v8::Object>,
) -> Result<(), String> {
    crate::live_layout_backend::capture_viewport(scope)?;
    let global = scope.get_current_context().global(scope);
    let name = v8::String::new(scope, "ts").ok_or("Geometry namespace allocation interrupted")?;
    let key = v8::Symbol::for_key(scope, name);
    let namespace = global
        .get(scope, key.into())
        .and_then(|value| value.to_object(scope))
        .ok_or("Geometry private namespace unavailable")?;
    let key = v8::String::new(scope, "makeNativeFunction")
        .ok_or("Geometry factory allocation interrupted")?;
    let factory = namespace
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or("Geometry native function factory unavailable")?;
    v8::tc_scope!(let scope,scope);
    let source = v8::String::new(scope, include_str!("geometry_backend.js"))
        .ok_or("Geometry source allocation interrupted")?;
    let initializer = v8::Script::compile(scope, source, None)
        .and_then(|script| script.run(scope))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
    if let Some(initializer) = initializer {
        let undefined = v8::undefined(scope);
        if let Some(factory) = initializer
            .call(scope, undefined.into(), &[core.into(), factory.into()])
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        {
            let key = factory_key(scope).ok_or("Geometry private key allocation interrupted")?;
            if global.set_private(scope, key, factory.into()) != Some(true) {
                return Err("Geometry private factory installation interrupted".into());
            }
            return Ok(());
        }
    }
    Err(scope
        .exception()
        .map(|error| error.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "Geometry installation interrupted".into()))
}

#[derive(Clone, Copy)]
pub(crate) struct RectData {
    pub(crate) values: [f64; 4],
    pub(crate) read_only: bool,
}
pub(crate) fn snapshot<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
) -> Option<RectData> {
    let record =
        v8::Local::<v8::Object>::try_from(crate::browser_env::get_interface_record(scope, object)?)
            .ok()?;
    let key = v8::String::new(scope, "kind")?;
    if record.has_own_property(scope, key.into()) != Some(true) {
        return None;
    }
    let kind = record.get(scope, key.into())?;
    if !kind.is_string() {
        return None;
    }
    let kind = kind.to_rust_string_lossy(scope);
    let read_only = match kind.as_str() {
        "DOMRect" => false,
        "DOMRectReadOnly" => true,
        _ => return None,
    };
    let mut values = [0.0; 4];
    for (index, name) in ["x", "y", "width", "height"].into_iter().enumerate() {
        let key = v8::String::new(scope, name)?;
        let value = record.get(scope, key.into())?;
        if !value.is_number() {
            return None;
        }
        values[index] = value.number_value(scope)?;
    }
    Some(RectData { values, read_only })
}
pub(crate) fn restore<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: RectData,
) -> Option<v8::Local<'s, v8::Object>> {
    let global = scope.get_current_context().global(scope);
    let key = factory_key(scope)?;
    let factory = global
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())?;
    let numbers = data
        .values
        .map(|value| v8::Number::new(scope, value).into());
    let read_only = v8::Boolean::new(scope, data.read_only);
    let values = [
        numbers[0],
        numbers[1],
        numbers[2],
        numbers[3],
        read_only.into(),
    ];
    let undefined = v8::undefined(scope);
    factory
        .call(scope, undefined.into(), &values)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
}
