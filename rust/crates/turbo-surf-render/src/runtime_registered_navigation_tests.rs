//! Registered native navigation helpers retain real receiver and retirement contracts.
use super::*;

async fn page() -> PageSession {
    PageSession::open(
        "<body><p id=stable>owner</p><iframe id=outer srcdoc=\"&lt;body>&lt;iframe id=inner>&lt;/iframe>\"></iframe><iframe id=opaque sandbox=allow-scripts srcdoc='<body>opaque'></iframe></body>",
        "https://registered-navigation.test/root", "", "", 5000,
    ).await.unwrap()
}

fn guest_value(page: &mut PageSession, source: &str) -> v8::Global<v8::Value> {
    let _activation = page.rt.activation_key().enter();
    let target = page.replaced_root_target().unwrap();
    auxiliary_pages::execute_target(
        &mut page.rt,
        turbo_dom_browser_env::host_internals::AuxiliaryContextId(target),
        source,
        false,
    )
    .unwrap()
}

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, name).expect("native own data property")
}

fn registered<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    name: &str,
) -> v8::Local<'s, v8::Function> {
    assert!(matches!(
        name,
        "op_frame_owner_element"
            | "op_window_parent"
            | "op_window_top"
            | "op_window_child_frames"
            | "op_window_opaque"
            | "op_window_active"
            | "op_native_location_installed"
            | "op_native_window_proxy"
            | "op_producer_window_proxy"
            | "op_location_seed"
    ));
    let core = root
        .get_slot::<RuntimeCore>()
        .expect("real captured runtime core");
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = own(scope, core, "ops");
    let ops = v8::Local::<v8::Object>::try_from(ops).unwrap();
    let function = own(scope, ops, name);
    v8::Local::<v8::Function>::try_from(function).expect("actual registered native function")
}

fn invoke(
    page: &mut PageSession,
    name: &str,
    args: &[v8::Global<v8::Value>],
) -> Result<v8::Global<v8::Value>, String> {
    let _activation = page.rt.activation_key().enter();
    let root = private_infrastructure::context(&mut page.rt).unwrap();
    deno_core::scope!(scope, &mut page.rt);
    let root = v8::Local::new(scope, root);
    let function = registered(scope, root, name);
    let scope = &mut v8::ContextScope::new(scope, root);
    v8::tc_scope!(let scope,scope);
    let values: Vec<_> = args
        .iter()
        .map(|value| v8::Local::new(scope, value))
        .collect();
    let undefined = v8::undefined(scope);
    if let Some(value) = function.call(scope, undefined.into(), &values) {
        assert!(!scope.has_caught());
        return Ok(v8::Global::new(scope, value));
    }
    assert!(
        !scope.has_terminated(),
        "native boundary fixture must not terminate execution"
    );
    let error = scope.exception().expect("native rejection exception");
    assert!(error.is_native_error());
    let error = v8::Local::<v8::Object>::try_from(error).unwrap();
    Err(error.get_constructor_name().to_rust_string_lossy(scope))
}

fn assert_same(
    page: &mut PageSession,
    actual: v8::Global<v8::Value>,
    expected: &v8::Global<v8::Value>,
) {
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let actual = v8::Local::new(scope, actual);
    let expected = v8::Local::new(scope, expected);
    assert!(actual.strict_equals(expected));
}

fn assert_null(page: &mut PageSession, value: v8::Global<v8::Value>) {
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    assert!(v8::Local::new(scope, value).is_null());
}

fn assert_bool(page: &mut PageSession, value: v8::Global<v8::Value>, expected: bool) {
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let value = v8::Local::new(scope, value);
    assert!(value.is_boolean());
    assert_eq!(value.boolean_value(scope), expected);
}

fn document(page: &mut PageSession, window: &v8::Global<v8::Value>) -> v8::Global<v8::Value> {
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let window = v8::Local::new(scope, window);
    let window = v8::Local::<v8::Object>::try_from(window).unwrap();
    let owner = window.get_creation_context(scope).unwrap();
    assert!(owner.global(scope).strict_equals(window.into()));
    let document = owner
        .get_slot::<ActiveDocument>()
        .unwrap()
        .0
        .to_local(scope)
        .unwrap();
    assert_eq!(document.get_creation_context(scope), Some(owner));
    v8::Global::new(scope, v8::Local::<v8::Value>::from(document))
}

fn assert_children(
    page: &mut PageSession,
    owner: &v8::Global<v8::Value>,
    expected: &[v8::Global<v8::Value>],
) {
    let result = invoke(page, "op_window_child_frames", std::slice::from_ref(owner)).unwrap();
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let result = v8::Local::new(scope, result);
    let array = v8::Local::<v8::Array>::try_from(result).unwrap();
    assert_eq!(array.length() as usize, expected.len());
    for (index, expected) in expected.iter().enumerate() {
        let value = array.get_index(scope, index as u32).unwrap();
        let expected = v8::Local::new(scope, expected);
        assert!(value.strict_equals(expected));
    }
}

async fn healthy(page: &mut PageSession) {
    let actual = page
        .eval_json_expression("[document.querySelector('#stable').textContent,typeof Deno]")
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!(["owner", "undefined"])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn registered_frame_owner_element_rejects_nonwindows_and_preserves_real_sop_results() {
    let mut page = page().await;
    let root = guest_value(&mut page, "globalThis");
    let outer = guest_value(&mut page, "document.querySelector('#outer').contentWindow");
    let host = guest_value(&mut page, "document.querySelector('#outer')");
    let opaque = guest_value(&mut page, "document.querySelector('#opaque').contentWindow");
    let root_document = document(&mut page, &root);
    let result = invoke(
        &mut page,
        "op_frame_owner_element",
        std::slice::from_ref(&root),
    )
    .unwrap();
    assert_null(&mut page, result);
    let result = invoke(
        &mut page,
        "op_frame_owner_element",
        std::slice::from_ref(&outer),
    )
    .unwrap();
    assert_same(&mut page, result, &host);
    let result = invoke(
        &mut page,
        "op_frame_owner_element",
        std::slice::from_ref(&opaque),
    )
    .unwrap();
    assert_null(&mut page, result);
    let plain = guest_value(&mut page, "({})");
    let primitive = guest_value(&mut page, "null");
    for invalid in [plain, root_document, host.clone(), primitive] {
        assert_eq!(
            invoke(&mut page, "op_frame_owner_element", &[invalid]).unwrap_err(),
            "TypeError"
        );
        let result = invoke(
            &mut page,
            "op_frame_owner_element",
            std::slice::from_ref(&outer),
        )
        .unwrap();
        assert_same(&mut page, result, &host);
    }
    guest_value(
        &mut page,
        "document.querySelector('#outer').remove();undefined",
    );
    let result = invoke(&mut page, "op_frame_owner_element", &[outer]).unwrap();
    assert_null(&mut page, result);
    healthy(&mut page).await;
}

#[tokio::test(flavor = "current_thread")]
async fn registered_topology_reads_follow_real_parentage_and_retire_only_the_removed_subtree() {
    let mut page = page().await;
    let root = guest_value(&mut page, "globalThis");
    let outer = guest_value(&mut page, "document.querySelector('#outer').contentWindow");
    let inner = guest_value(
        &mut page,
        "document.querySelector('#outer').contentDocument.querySelector('#inner').contentWindow",
    );
    let opaque = guest_value(&mut page, "document.querySelector('#opaque').contentWindow");
    let root_document = document(&mut page, &root);
    let outer_document = document(&mut page, &outer);
    for (window, parent) in [
        (&root, &root),
        (&outer, &root),
        (&inner, &outer),
        (&opaque, &root),
    ] {
        let result = invoke(&mut page, "op_window_parent", std::slice::from_ref(window)).unwrap();
        assert_same(&mut page, result, parent);
        let result = invoke(&mut page, "op_window_top", std::slice::from_ref(window)).unwrap();
        assert_same(&mut page, result, &root);
    }
    assert_children(&mut page, &root_document, &[outer.clone(), opaque.clone()]);
    assert_children(&mut page, &outer_document, std::slice::from_ref(&inner));
    for (window, expected) in [(&root, false), (&outer, false), (&opaque, true)] {
        let result = invoke(&mut page, "op_window_opaque", std::slice::from_ref(window)).unwrap();
        assert_bool(&mut page, result, expected);
    }
    for name in ["op_native_location_installed", "op_producer_window_proxy"] {
        let result = invoke(&mut page, name, std::slice::from_ref(&root_document)).unwrap();
        assert_bool(&mut page, result, true);
    }
    let result = invoke(&mut page, "op_native_window_proxy", &[]).unwrap();
    assert_bool(&mut page, result, true);
    guest_value(
        &mut page,
        "document.querySelector('#outer').remove();undefined",
    );
    for window in [&outer, &inner] {
        let result = invoke(&mut page, "op_window_active", std::slice::from_ref(window)).unwrap();
        assert_bool(&mut page, result, false);
        for name in ["op_window_parent", "op_window_top"] {
            let result = invoke(&mut page, name, std::slice::from_ref(window)).unwrap();
            assert_null(&mut page, result);
        }
    }
    for window in [&root, &opaque] {
        let result = invoke(&mut page, "op_window_active", std::slice::from_ref(window)).unwrap();
        assert_bool(&mut page, result, true);
    }
    assert_children(&mut page, &root_document, &[opaque]);
    healthy(&mut page).await;
}

fn native_url(page: &mut PageSession, window: &v8::Global<v8::Value>) -> String {
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let value = v8::Local::new(scope, window);
    let window = v8::Local::<v8::Object>::try_from(value).unwrap();
    let context = window.get_creation_context(scope).unwrap();
    window_proxy::location_url(context).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn registered_location_seed_changes_live_metadata_and_rejects_a_real_retired_document() {
    let mut page = page().await;
    let root = guest_value(&mut page, "globalThis");
    let outer = guest_value(&mut page, "document.querySelector('#outer').contentWindow");
    let root_document = document(&mut page, &root);
    let child_document = document(&mut page, &outer);
    let old_child_url = native_url(&mut page, &outer);
    let root_id = page.auxiliary_root_id().unwrap();
    let admission = page
        .network_event_log()
        .target_network_state(&root_id.to_string())["navigationRevision"]
        .clone();
    assert!(
        admission.is_string(),
        "real Root admission tracking must be installed"
    );
    let seeded = guest_value(
        &mut page,
        "'https://registered-navigation.test/root#seeded'",
    );
    let result = invoke(
        &mut page,
        "op_location_seed",
        &[root_document.clone(), seeded.clone()],
    )
    .unwrap();
    assert_bool(&mut page, result, true);
    assert_eq!(
        native_url(&mut page, &root),
        "https://registered-navigation.test/root#seeded"
    );
    guest_value(
        &mut page,
        "document.querySelector('#outer').remove();undefined",
    );
    let result = invoke(
        &mut page,
        "op_location_seed",
        &[child_document, seeded.clone()],
    )
    .unwrap();
    assert_bool(&mut page, result, false);
    assert_eq!(native_url(&mut page, &outer), old_child_url);
    assert_eq!(
        native_url(&mut page, &root),
        "https://registered-navigation.test/root#seeded"
    );
    let primitive = guest_value(&mut page, "null");
    assert_eq!(
        invoke(&mut page, "op_location_seed", &[primitive, seeded.clone()]).unwrap_err(),
        "TypeError"
    );
    assert_eq!(
        native_url(&mut page, &root),
        "https://registered-navigation.test/root#seeded"
    );
    let result = invoke(&mut page, "op_location_seed", &[root_document, seeded]).unwrap();
    assert_bool(&mut page, result, true);
    assert_eq!(
        page.network_event_log()
            .target_network_state(&root_id.to_string())["navigationRevision"],
        admission
    );
    assert!(
        !page.has_pending_auxiliary_work(),
        "a metadata seed does not admit document navigation"
    );
    healthy(&mut page).await;
}
