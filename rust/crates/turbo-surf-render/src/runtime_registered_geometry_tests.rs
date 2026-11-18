//! Registered functions remain host-local; guest scripts receive only DOM values.
use super::*;

async fn page(html: &str) -> PageSession {
    PageSession::open(html, "https://native-geometry.test/", "", "", 5000)
        .await
        .unwrap()
}

macro_rules! root_scope {
    ($page:ident, $scope:ident, $root:ident) => {
        let _activation = $page.rt.activation_key().enter();
        let selected = private_infrastructure::context(&mut $page.rt).unwrap();
        deno_core::scope!($scope, &mut $page.rt);
        let $root = v8::Local::new($scope, selected);
        let $scope = &mut v8::ContextScope::new($scope, $root);
    };
}

fn own_data<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    let key = v8::String::new(scope, name).unwrap();
    let descriptor = object
        .get_own_property_descriptor(scope, key.into())
        .unwrap();
    let descriptor =
        v8::Local::<v8::Object>::try_from(descriptor).expect("registered data is an own property");
    let key = v8::String::new(scope, "value").unwrap();
    assert_eq!(descriptor.has_own_property(scope, key.into()), Some(true));
    descriptor.get(scope, key.into()).unwrap()
}

fn registered<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    name: &str,
) -> v8::Local<'s, v8::Function> {
    assert!(
        matches!(
            name,
            "op_live_element_geometry"
                | "op_live_elements_at_point"
                | "op_live_element_offset"
                | "op_live_range_geometry"
                | "op_live_caret_at_point"
                | "op_live_caret_geometry"
                | "op_live_frame_dimension"
                | "op_request_frame_rendering"
        ),
        "only the reviewed registered geometry operations may be called"
    );
    let core = root.get_slot::<RuntimeCore>().expect("actual guest core");
    let core = core.0.to_local(scope).expect("runtime owns its real core");
    let operations = own_data(scope, core, "ops").to_object(scope).unwrap();
    v8::Local::<v8::Function>::try_from(own_data(scope, operations, name))
        .expect("actual registered operation")
}

fn call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    function: v8::Local<'s, v8::Function>,
    args: &[v8::Local<'s, v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, String> {
    v8::tc_scope!(let scope,scope);
    let undefined = v8::undefined(scope);
    function.call(scope, undefined.into(), args).ok_or_else(|| {
        scope
            .exception()
            .map(|error| error.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "native call interrupted".to_owned())
    })
}

fn expression<'s>(scope: &mut v8::PinScope<'s, '_>, source: &str) -> v8::Local<'s, v8::Value> {
    v8::tc_scope!(let scope,scope);
    let source = v8::String::new(scope, source).unwrap();
    v8::Script::compile(scope, source, None)
        .and_then(|script| script.run(scope))
        .expect("ordinary DOM expression")
}

fn dom_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: &str,
    values: &[v8::Local<'s, v8::Value>],
) -> v8::Local<'s, v8::Value> {
    let function = v8::Local::<v8::Function>::try_from(expression(scope, source)).unwrap();
    call(scope, function, values).expect("DOM-only guest function")
}

fn entered_root(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let entered = scope.get_entered_or_microtask_context();
    result.set_bool(args.data().strict_equals(entered.global(scope).into()));
}

fn assert_root_entry<'s>(scope: &mut v8::PinScope<'s, '_>, root: v8::Local<'s, v8::Context>) {
    let window = root.global(scope);
    let probe = v8::Function::builder(entered_root)
        .data(window.into())
        .build(scope)
        .unwrap();
    assert!(
        call(scope, probe, &[]).unwrap().is_true(),
        "actual V8 entry must be Root"
    );
}

fn type_error(result: Result<v8::Local<v8::Value>, String>) {
    let error = match result {
        Ok(_) => panic!("wrong native receiver was accepted"),
        Err(error) => error,
    };
    assert!(error.starts_with("TypeError:"), "{error}");
}

fn json<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> serde_json::Value {
    deno_core::serde_v8::from_v8(scope, value).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_native_node_brands_fail_without_poisoning_geometry_reads() {
    let mut page = page("<body><div id=box style='position:absolute;left:10px;top:20px;width:120px;height:40px'>text</div></body>").await;
    root_scope!(page, scope, root);
    assert_root_entry(scope, root);
    let document = expression(scope, "document");
    let element = expression(scope, "document.getElementById('box')");
    let before = expression(scope, "document.body.innerHTML").to_rust_string_lossy(scope);
    let geometry = registered(scope, root, "op_live_element_geometry");
    let false_value = v8::Boolean::new(scope, false).into();
    let true_value = v8::Boolean::new(scope, true).into();
    type_error(call(scope, geometry, &[document, false_value]));
    let result = call(scope, geometry, &[element, true_value]).unwrap();
    assert_eq!(json(scope, result)["bounds"][2], 120);

    let hit = registered(scope, root, "op_live_elements_at_point");
    let x = v8::Number::new(scope, 15.0).into();
    let y = v8::Number::new(scope, 25.0).into();
    type_error(call(scope, hit, &[element, x, y]));
    let result = call(scope, hit, &[document, x, y]).unwrap();
    let result = v8::Local::<v8::Array>::try_from(result).unwrap();
    assert!((0..result.length()).any(|index| result
        .get_index(scope, index)
        .unwrap()
        .strict_equals(element)));

    let offset = registered(scope, root, "op_live_element_offset");
    type_error(call(scope, offset, &[document]));
    let result = call(scope, offset, &[element])
        .unwrap()
        .to_object(scope)
        .unwrap();
    assert_eq!(
        own_data(scope, result, "left").number_value(scope),
        Some(10.0)
    );
    assert_eq!(
        own_data(scope, result, "top").number_value(scope),
        Some(20.0)
    );

    let ranges = registered(scope, root, "op_live_range_geometry");
    type_error(call(scope, ranges, &[element]));
    let range = expression(scope, "(() => {const range=document.createRange();range.selectNodeContents(document.getElementById('box'));return range})()");
    let result = call(scope, ranges, &[range]).unwrap();
    assert!(!json(scope, result)["rects"].as_array().unwrap().is_empty());
    assert_eq!(
        expression(scope, "document.body.innerHTML").to_rust_string_lossy(scope),
        before
    );
    assert_root_entry(scope, root);
}

#[tokio::test(flavor = "current_thread")]
async fn native_caret_boundaries_distinguish_inert_document_and_non_text_source() {
    let mut page = page("<body><p id=text>caret text</p></body>").await;
    root_scope!(page, scope, root);
    assert_root_entry(scope, root);
    let before = expression(scope, "document.body.innerHTML").to_rust_string_lossy(scope);
    let inert = expression(scope, "(() => {const doc=document.implementation.createHTMLDocument('inert');doc.body.textContent='not in a viewport';return doc})()");
    let paragraph = expression(scope, "document.getElementById('text')");
    let text = expression(scope, "document.getElementById('text').firstChild");
    let document = expression(scope, "document");
    let at_point = registered(scope, root, "op_live_caret_at_point");
    let geometry = registered(scope, root, "op_live_caret_geometry");
    let one = v8::Number::new(scope, 1.0).into();
    let zero = v8::Integer::new_from_unsigned(scope, 0).into();
    assert!(call(scope, at_point, &[inert, one, one]).unwrap().is_null());
    let result = call(scope, geometry, &[paragraph, zero]).unwrap();
    assert_eq!(json(scope, result)["rect"], serde_json::json!([0, 0, 0, 0]));
    let result = call(scope, geometry, &[text, zero]).unwrap();
    let caret = json(scope, result);
    assert!(caret["rect"][3].as_f64().unwrap() > 0.0);
    let point = expression(scope, "(() => {const r=document.getElementById('text').getBoundingClientRect();return [r.x+1,r.y+r.height/2]})()");
    let point = v8::Local::<v8::Array>::try_from(point).unwrap();
    let x = point.get_index(scope, 0).unwrap();
    let y = point.get_index(scope, 1).unwrap();
    let result = call(scope, at_point, &[document, x, y])
        .unwrap()
        .to_object(scope)
        .unwrap();
    assert!(own_data(scope, result, "node").strict_equals(text));
    let inert_text = dom_function(
        scope,
        "(function(doc){return doc.body.textContent})",
        &[inert],
    );
    assert_eq!(inert_text.to_rust_string_lossy(scope), "not in a viewport");
    assert_eq!(
        expression(scope, "document.body.innerHTML").to_rust_string_lossy(scope),
        before
    );
}

fn frame(
    page: &mut PageSession,
    name: &str,
) -> (
    v8::Global<v8::Context>,
    v8::Global<v8::Value>,
    v8::Global<v8::Value>,
) {
    let _activation = page.rt.activation_key().enter();
    let frames = page
        .rt
        .op_state()
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    deno_core::scope!(scope, &mut page.rt);
    for frame in frames {
        let context = v8::Local::new(scope, frame);
        if context.get_slot::<FrameParent>().is_none()
            || context.get_slot::<FrameRetired>().is_some()
        {
            continue;
        }
        let scope = &mut v8::ContextScope::new(scope, context);
        if expression(scope, "name").to_rust_string_lossy(scope) != name {
            continue;
        }
        let window = context.global(scope);
        let document = expression(scope, "document");
        return (
            v8::Global::new(scope, context),
            v8::Global::new(scope, v8::Local::<v8::Value>::from(window)),
            v8::Global::new(scope, document),
        );
    }
    panic!("real initialized child frame {name} missing");
}

#[tokio::test(flavor = "current_thread")]
async fn native_window_dimension_read_flags_and_actual_cross_origin_tokens_are_enforced() {
    let mut page = page("<body><iframe name=opaque sandbox=allow-scripts srcdoc='<p>opaque child</p>'></iframe></body>").await;
    let (opaque_context, opaque, _) = frame(&mut page, "opaque");
    root_scope!(page, scope, root);
    assert_root_entry(scope, root);
    let opaque_context = v8::Local::new(scope, &opaque_context);
    let root_token = root.get_security_token(scope);
    let opaque_token = opaque_context.get_security_token(scope);
    assert!(
        !root_token.strict_equals(opaque_token),
        "actual sandbox child has a foreign token"
    );
    let opaque = v8::Local::new(scope, &opaque);
    let window = root.global(scope).into();
    let document = expression(scope, "document");
    let before = expression(scope, "[innerWidth,innerHeight,document.body.innerHTML]");
    let before = json(scope, before);
    let dimension = registered(scope, root, "op_live_frame_dimension");
    let yes = v8::Boolean::new(scope, true).into();
    let no = v8::Boolean::new(scope, false).into();
    for height in [no, yes] {
        assert_eq!(
            call(scope, dimension, &[window, height, no])
                .unwrap()
                .number_value(scope),
            Some(0.0)
        );
    }
    assert_eq!(
        call(scope, dimension, &[window, no, yes])
            .unwrap()
            .number_value(scope),
        before[0].as_f64()
    );
    assert_eq!(
        call(scope, dimension, &[window, yes, yes])
            .unwrap()
            .number_value(scope),
        before[1].as_f64()
    );
    assert!(call(scope, dimension, &[opaque, no, yes])
        .unwrap()
        .is_null());
    assert!(call(scope, dimension, &[opaque, yes, yes])
        .unwrap()
        .is_null());
    type_error(call(scope, dimension, &[document, no, yes]));
    let after = expression(scope, "[innerWidth,innerHeight,document.body.innerHTML]");
    assert_eq!(json(scope, after), before);
    assert_eq!(
        call(scope, dimension, &[window, no, yes])
            .unwrap()
            .number_value(scope),
        before[0].as_f64()
    );
    assert_root_entry(scope, root);
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_and_retired_rendering_targets_preserve_documents_and_live_frame_progress() {
    let mut page = page("<body><iframe id=live name=live style='width:200px;height:100px' srcdoc='<p>live child</p>'></iframe><iframe id=retired name=retired srcdoc='<p>retired child marker</p>'></iframe></body>").await;
    let (_, live, _) = frame(&mut page, "live");
    let (retired_context, retired, retired_document) = frame(&mut page, "retired");
    page.eval("document.getElementById('retired').remove()")
        .await
        .unwrap();
    {
        root_scope!(page, scope, root);
        assert_root_entry(scope, root);
        let retired_context = v8::Local::new(scope, &retired_context);
        assert!(
            retired_context.get_slot::<FrameRetired>().is_some(),
            "real DOM removal retires the child first"
        );
        let retired = v8::Local::new(scope, &retired);
        let retired_document = v8::Local::new(scope, &retired_document);
        let live = v8::Local::new(scope, &live);
        let before = dom_function(
            scope,
            "(function(doc){return doc.body.textContent})",
            &[retired_document],
        )
        .to_rust_string_lossy(scope);
        let root_before = expression(scope, "document.body.innerHTML").to_rust_string_lossy(scope);
        let render = registered(scope, root, "op_request_frame_rendering");
        let document = expression(scope, "document");
        // Unmarked Rust unit uses Deno's null ABI; the caller discards it.
        assert!(call(scope, render, &[document]).unwrap().is_null());
        assert!(call(scope, render, &[retired]).unwrap().is_null());
        assert_eq!(
            dom_function(
                scope,
                "(function(doc){return doc.body.textContent})",
                &[retired_document]
            )
            .to_rust_string_lossy(scope),
            before
        );
        assert_eq!(before, "retired child marker");
        assert_eq!(
            expression(scope, "document.body.innerHTML").to_rust_string_lossy(scope),
            root_before
        );
        dom_function(
            scope,
            r#"(function(live) {
          globalThis.renderingWitness={done:false,width:0};
          document.getElementById('live').style.width='320px';
          live.requestAnimationFrame(()=>{renderingWitness.width=live.innerWidth;renderingWitness.done=true});
          return true;
        })"#,
            &[live],
        );
        assert!(call(scope, render, &[live]).unwrap().is_null());
        assert_root_entry(scope, root);
    }
    page.eval_engine("__tsNs.runTimers(200)").await.unwrap();
    assert_eq!(
        page.eval_json_expression("[renderingWitness.done,renderingWitness.width]")
            .await
            .unwrap(),
        "[true,320]"
    );
    assert_eq!(
        page.eval_json_expression(
            "document.getElementById('live').contentDocument.body.textContent"
        )
        .await
        .unwrap(),
        "\"live child\""
    );
}
