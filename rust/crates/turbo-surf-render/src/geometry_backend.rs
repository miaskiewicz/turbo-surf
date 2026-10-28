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
    crate::live_layout_dom::valid_receiver(scope, receiver, kind)
}
fn frame_scheduler_key<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, "turbo-surf.frame.scheduler")?;
    Some(v8::Private::for_api(scope, Some(name)))
}
/// Trusted bootstrap hands the realm's rendering-frame scheduler to geometry
/// through a V8 private on the global; page script can neither read nor call it.
#[op2(fast, reentrant)]
fn op_capture_frame_scheduler<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    scheduler: v8::Local<'s, v8::Function>,
) -> Result<(), JsErrorBox> {
    // Bind to the scheduler's own realm: frame bootstraps can run while the
    // parent context is current, and a parent-held child closure would keep a
    // retired frame realm alive.
    let global = scheduler
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("Frame scheduler realm unavailable"))?
        .global(scope);
    let key = frame_scheduler_key(scope)
        .ok_or_else(|| JsErrorBox::generic("Frame scheduler key allocation interrupted"))?;
    if global.set_private(scope, key, scheduler.into()) != Some(true) {
        return Err(JsErrorBox::generic(
            "Frame scheduler installation interrupted",
        ));
    }
    Ok(())
}
/// Trusted bootstrap hands the realm's view-transition rendering seam (frame steps after
/// animation callbacks, rendering suppression, page visibility) to the binding.
#[op2(fast, reentrant)]
fn op_capture_view_transition_host<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host: v8::Local<'s, v8::Function>,
) {
    crate::browser_env::capture_view_transition_host(scope, host);
}
/// The runtime replaces the binding's DOMException with its own interface; natively
/// raised exceptions (selectors, dialogs, view transitions) must be instances of it.
#[op2(fast, reentrant)]
fn op_register_dom_exception<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    constructor: v8::Local<'s, v8::Function>,
) {
    crate::browser_env::register_dom_exception(scope, constructor);
}
deno_core::extension!(
    turbo_geometry,
    ops = [
        op_geometry_object,
        op_install_geometry_clone_adapter,
        op_capture_frame_scheduler,
        op_capture_view_transition_host,
        op_register_dom_exception,
        crate::live_layout_backend::op_live_media_match,
        crate::live_layout_backend::op_live_frame_dimension,
        crate::live_layout_supports::op_native_css_supports,
        op_live_geometry_brand,
        crate::live_layout_backend::op_live_element_geometry,
        crate::live_layout_backend::op_live_image_box,
        crate::live_layout_backend::op_live_image_loaded,
        crate::live_layout_backend::op_live_element_scroll,
        crate::live_layout_backend::op_live_element_scroll_to,
        crate::live_layout_backend::op_live_viewport_offset,
        crate::live_layout_backend::op_live_scroll_into_view,
        crate::live_layout_backend::op_live_element_visibility,
        crate::live_layout_backend::op_live_element_zoom,
        crate::live_layout_backend::op_live_scroll_container,
        crate::live_layout_backend::op_live_input_size,
        crate::live_layout_backend::op_live_elements_at_point,
        crate::live_layout_backend::op_live_element_offset,
        crate::live_layout_backend::op_live_range_geometry,
        crate::live_layout_backend::op_live_caret_at_point,
        crate::live_layout_backend::op_live_caret_geometry
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
    crate::live_layout_backend::initialize_system_fonts();
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
        let context = scope.get_current_context();
        let frame_window = crate::runtime::native_frame_layout_host(scope, context).is_some();
        let frame_window = v8::Boolean::new(scope, frame_window);
        let scheduler = frame_scheduler_key(scope)
            .and_then(|key| global.get_private(scope, key))
            .filter(|value| value.is_function())
            .unwrap_or_else(|| v8::undefined(scope).into());
        if let Some(factory) = initializer
            .call(
                scope,
                undefined.into(),
                &[core.into(), factory.into(), frame_window.into(), scheduler],
            )
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        {
            let key = factory_key(scope).ok_or("Geometry private key allocation interrupted")?;
            if global.set_private(scope, key, factory.into()) != Some(true) {
                return Err("Geometry private factory installation interrupted".into());
            }
            hand_over_quad_rectangle(scope, global, namespace);
            return Ok(());
        }
    }
    Err(scope
        .exception()
        .map(|error| error.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "Geometry installation interrupted".into()))
}

/// DOMQuad.getBounds (web_geometry.js, bootstrapped before DOMRect exists) gets
/// the realm's DOMRect constructor once, before any page script runs.
fn hand_over_quad_rectangle<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    global: v8::Local<'s, v8::Object>,
    namespace: v8::Local<'s, v8::Object>,
) {
    let read = |scope: &mut v8::PinScope<'s, '_>, object: v8::Local<'s, v8::Object>, name: &str| {
        let key = v8::String::new(scope, name)?;
        object
            .get(scope, key.into())
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    };
    let (Some(rectangle), Some(hook)) = (
        read(scope, global, "DOMRect"),
        read(scope, namespace, "installQuadRectangle"),
    ) else {
        return;
    };
    let undefined = v8::undefined(scope);
    hook.call(scope, undefined.into(), &[rectangle.into()]);
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

/// The realm's private DOMPoint/DOMMatrix clone hooks (web_geometry.js), retained
/// weakly; the realm owns the functions.
pub(crate) struct GeometryCloneAdapter {
    record: v8::Weak<v8::Function>,
    restore: v8::Weak<v8::Function>,
}

#[op2(fast)]
fn op_install_geometry_clone_adapter(
    scope: &mut v8::PinScope,
    record: v8::Local<v8::Function>,
    restore: v8::Local<v8::Function>,
) {
    let Some(context) = record.get_creation_context(scope) else {
        return;
    };
    let Some(record) = crate::context_ownership::function(scope, record) else {
        return;
    };
    let Some(restore) = crate::context_ownership::function(scope, restore) else {
        return;
    };
    context.set_slot(std::rc::Rc::new(GeometryCloneAdapter { record, restore }));
}

/// A DOMPointReadOnly (1), DOMPoint (2), DOMMatrixReadOnly (3), DOMMatrix (4) or
/// DOMQuad (5) platform object's values: 4 point coordinates, 16 column-major
/// entries, or the quad's four points' x, y, z, w.
#[derive(Clone)]
pub(crate) struct PointMatrixData {
    kind: u32,
    values: Vec<f64>,
}
impl PointMatrixData {
    pub(crate) fn size_bytes(&self) -> usize {
        8 * self.values.len() + 8
    }
}

/// Reads a point/matrix through its own realm's hook; anything else (a plain or
/// prototype-only object, a foreign realm without hooks, malformed hook output) is
/// `None`. Callers provide a scope that allows JavaScript execution.
pub(crate) fn point_matrix_snapshot<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
) -> Option<PointMatrixData> {
    let context = object.get_creation_context(scope)?;
    let record = context
        .get_slot::<GeometryCloneAdapter>()?
        .record
        .to_local(scope)?;
    let undefined = v8::undefined(scope);
    let result = record.call(scope, undefined.into(), &[object.into()])?;
    let result = v8::Local::<v8::Array>::try_from(result).ok()?;
    let kind = result.get_index(scope, 0)?;
    let kind = match kind
        .is_uint32()
        .then(|| kind.uint32_value(scope))
        .flatten()?
    {
        kind @ 1..=5 => kind,
        _ => return None,
    };
    let data = v8::Local::<v8::Array>::try_from(result.get_index(scope, 1)?).ok()?;
    // Points carry 4 values, quads 16; a matrix carries 6 when it is 2D (its is2D
    // flag, turbo-surf#89) and 16 when it is 3D.
    let length = data.length();
    let valid = match kind {
        1 | 2 => length == 4,
        3 | 4 => length == 6 || length == 16,
        _ => length == 16,
    };
    if !valid {
        return None;
    }
    let mut values = Vec::with_capacity(length as usize);
    for index in 0..length {
        let value = data.get_index(scope, index)?;
        if !value.is_number() {
            return None;
        }
        values.push(value.number_value(scope)?);
    }
    Some(PointMatrixData { kind, values })
}

/// Rebuilds the platform object in the current realm, or `None` when that realm
/// has no geometry hooks.
pub(crate) fn point_matrix_restore<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: &PointMatrixData,
) -> Option<v8::Local<'s, v8::Object>> {
    let restore = scope
        .get_current_context()
        .get_slot::<GeometryCloneAdapter>()?
        .restore
        .to_local(scope)?;
    let values: Vec<v8::Local<v8::Value>> = data
        .values
        .iter()
        .map(|value| v8::Number::new(scope, *value).into())
        .collect();
    let values = v8::Array::new_with_elements(scope, &values);
    let kind = v8::Integer::new_from_unsigned(scope, data.kind);
    let undefined = v8::undefined(scope);
    restore
        .call(scope, undefined.into(), &[kind.into(), values.into()])?
        .to_object(scope)
}
