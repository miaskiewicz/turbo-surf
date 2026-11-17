use super::*;

async fn page() -> PageSession {
    PageSession::open(
        "<body><iframe id=a></iframe><iframe id=b></iframe></body>",
        "https://boundary.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

fn guest_value(page: &mut PageSession, source: &str) -> v8::Global<v8::Value> {
    let target = page.replaced_root_target().expect("native guest root");
    auxiliary_pages::execute_target(
        &mut page.rt,
        turbo_dom_browser_env::host_internals::AuxiliaryContextId(target),
        source,
        false,
    )
    .unwrap()
}

fn rehome_native(page: &mut PageSession, source: &str) -> Result<(), String> {
    let _activation = page.rt.activation_key().enter();
    let value = page
        .rt
        .execute_script("<native-boundary-test>", source.to_owned())
        .unwrap();
    rehome_value(page, value)
}

fn rehome_value(page: &mut PageSession, value: v8::Global<v8::Value>) -> Result<(), String> {
    let guest = private_infrastructure::context(&mut page.rt).unwrap();
    deno_core::scope!(scope, page.rt);
    let guest = v8::Local::new(scope, guest);
    let scope = &mut v8::ContextScope::new(scope, guest);
    let value = v8::Local::new(scope, value);
    assert!(infrastructure(scope, value));
    let original = value;
    let value = rehome(scope, value).map_err(|error| error.to_string())?;
    assert!(value.strict_equals(original));
    assert!(!infrastructure(scope, value));
    let object = v8::Local::<v8::Object>::try_from(value).unwrap();
    let prototype = object.get_prototype(scope).unwrap();
    let prototype = v8::Local::<v8::Object>::try_from(prototype).unwrap();
    assert_eq!(prototype.get_creation_context(scope), Some(guest));
    Ok(())
}

// The private infrastructure intentionally has no authored SharedArrayBuffer
// constructor. Native factories test the guard without exposing that capability.
fn shared_native_value(page: &mut PageSession, view: bool) -> v8::Global<v8::Value> {
    deno_core::scope!(scope, page.rt);
    let infrastructure = scope.get_current_context();
    assert!(infrastructure.get_slot::<Infrastructure>().is_some());
    let buffer = v8::SharedArrayBuffer::new(scope, 8).unwrap();
    if !view {
        let value: v8::Local<v8::Value> = buffer.into();
        return v8::Global::new(scope, value);
    }
    let name = v8::String::new(scope, "Uint8Array").unwrap();
    let constructor = infrastructure
        .global(scope)
        .get(scope, name.into())
        .unwrap();
    let constructor = v8::Local::<v8::Function>::try_from(constructor).unwrap();
    let value = constructor.new_instance(scope, &[buffer.into()]).unwrap();
    assert!(value.is_uint8_array());
    let value: v8::Local<v8::Value> = value.into();
    v8::Global::new(scope, value)
}

#[tokio::test(flavor = "current_thread")]
async fn every_native_byte_view_is_rehomed_to_the_actual_guest_intrinsics() {
    let mut page = page().await;
    for name in [
        "Uint8Array",
        "Uint8ClampedArray",
        "Int8Array",
        "Uint16Array",
        "Int16Array",
        "Uint32Array",
        "Int32Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "BigUint64Array",
        "BigInt64Array",
    ] {
        rehome_native(&mut page, &format!("new {name}(2)")).unwrap();
    }
    rehome_native(&mut page, "new DataView(new ArrayBuffer(8))").unwrap();
    rehome_native(&mut page, "new ArrayBuffer(8)").unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn unsafe_native_values_are_rejected_without_poisoning_guest_reads() {
    let mut page = page().await;
    for source in [
        "new Proxy(new Uint8Array(1),{})",
        "Object.assign(new Uint8Array(1),{secret:1})",
        "({})",
        "new EvalError('unsupported')",
    ] {
        let error = rehome_native(&mut page, source).unwrap_err();
        assert!(
            error.contains("Unsupported private infrastructure value"),
            "{source}: {error}"
        );
    }
    for view in [false, true] {
        let _activation = page.rt.activation_key().enter();
        let value = shared_native_value(&mut page, view);
        let error = rehome_value(&mut page, value).unwrap_err();
        assert!(
            error.contains("Unsupported private infrastructure value"),
            "{error}"
        );
    }
    assert_eq!(
        page.eval_expression("document.querySelectorAll('iframe').length")
            .await
            .unwrap(),
        "2"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn guest_values_keep_identity_and_frame_authority_requires_the_actual_parent() {
    let mut page = page().await;
    let _activation = page.rt.activation_key().enter();
    let authored = guest_value(&mut page, "globalThis.marker={value:42};marker");
    let child = guest_value(&mut page, "document.querySelector('#a').contentWindow");
    let sibling = guest_value(&mut page, "document.querySelector('#b').contentWindow");
    let guest = private_infrastructure::context(&mut page.rt).unwrap();
    deno_core::scope!(scope, page.rt);
    let guest = v8::Local::new(scope, guest);
    let scope = &mut v8::ContextScope::new(scope, guest);
    let authored = v8::Local::new(scope, authored);
    assert!(rehome(scope, authored).unwrap().strict_equals(authored));
    let child = v8::Local::<v8::Object>::try_from(v8::Local::new(scope, child)).unwrap();
    let sibling = v8::Local::<v8::Object>::try_from(v8::Local::new(scope, sibling)).unwrap();
    assert!(frame_owner(scope, guest, child).is_ok());
    let wrong_parent = sibling.get_creation_context(scope).unwrap();
    assert!(frame_owner(scope, wrong_parent, child).is_err());
    let root_window = guest.global(scope);
    assert!(frame_owner(scope, guest, root_window).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn captured_frame_capability_rejects_after_native_frame_retirement() {
    let mut page = page().await;
    let invoke = {
        let _activation = page.rt.activation_key().enter();
        let child = guest_value(&mut page, "document.querySelector('#a').contentWindow");
        deno_core::scope!(scope, page.rt);
        let child = v8::Local::<v8::Object>::try_from(v8::Local::new(scope, child)).unwrap();
        let context = child.get_creation_context(scope).unwrap();
        let scope = &mut v8::ContextScope::new(scope, context);
        let capability = capabilities(scope).unwrap();
        let name = v8::String::new(scope, "invoke").unwrap();
        let invoke = capability.get(scope, name.into()).unwrap();
        v8::Global::new(scope, v8::Local::<v8::Function>::try_from(invoke).unwrap())
    };
    page.eval_expression("document.querySelector('#a').remove();true")
        .await
        .unwrap();
    {
        let _activation = page.rt.activation_key().enter();
        let guest = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, page.rt);
        let guest = v8::Local::new(scope, guest);
        let scope = &mut v8::ContextScope::new(scope, guest);
        v8::tc_scope!(let scope, scope);
        let invoke = v8::Local::new(scope, invoke);
        let receiver = v8::undefined(scope);
        assert!(invoke.call(scope, receiver.into(), &[]).is_none());
        let error = scope.exception().expect("retired capability must throw");
        assert!(error
            .to_rust_string_lossy(scope)
            .contains("Private boundary document is closed or not a guest"));
    }
    assert_eq!(
        page.eval_expression("document.querySelectorAll('iframe').length")
            .await
            .unwrap(),
        "1"
    );
}
