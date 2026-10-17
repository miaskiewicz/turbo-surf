use super::*;

#[test]
fn inert_initializer_uses_its_creation_realm_with_shared_embedder_ops() {
    tests::init_v8();
    let isolate = &mut v8::Isolate::new(Default::default());
    {
        v8::scope!(let scope, isolate);
        let main = v8::Context::new(scope, Default::default());
        let child = v8::Context::new(scope, Default::default());
        let main_function = {
            let scope = &mut v8::ContextScope::new(scope, main);
            let source = v8::String::new(scope, "(function mainInitializer(){})").unwrap();
            let value = v8::Script::compile(scope, source, None)
                .unwrap()
                .run(scope)
                .unwrap();
            v8::Local::<v8::Function>::try_from(value).unwrap()
        };
        let child_function = {
            let scope = &mut v8::ContextScope::new(scope, child);
            let source = v8::String::new(scope, "(function childInitializer(){})").unwrap();
            let value = v8::Script::compile(scope, source, None)
                .unwrap()
                .run(scope)
                .unwrap();
            v8::Local::<v8::Function>::try_from(value).unwrap()
        };
        let scope = &mut v8::ContextScope::new(scope, main);
        assert!(dom_implementation::set_initializer(scope, main_function));
        assert!(dom_implementation::set_initializer(scope, child_function));
        let name = v8::String::new(scope, "turbo.native.inert-document.initializer").unwrap();
        let key = v8::Private::for_api(scope, Some(name));
        assert!(main
            .global(scope)
            .get_private(scope, key)
            .unwrap()
            .strict_equals(main_function.into()));
        assert!(child
            .global(scope)
            .get_private(scope, key)
            .unwrap()
            .strict_equals(child_function.into()));
    }
}
#[test]
fn independent_factories_and_constructors_match_authored_chrome155_control() {
    tests::init_v8();
    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope,isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    install_with_html(
        scope,
        "<!doctype html><html><head></head><body></body></html>",
    );
    let probe = include_str!("dom_implementation_probe.js");
    let source = format!("JSON.stringify({})", probe.trim().trim_end_matches(';'));
    v8::tc_scope!(let scope,scope);
    let source = v8::String::new(scope, &source).unwrap();
    let result = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
    let result = result.unwrap_or_else(|| {
        panic!(
            "factory probe failed: {:?}",
            scope
                .exception()
                .map(|error| error.to_rust_string_lossy(scope))
        )
    });
    let result: serde_json::Value =
        serde_json::from_str(&result.to_rust_string_lossy(scope)).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("dom_implementation_chrome155.json")).unwrap();
    assert_eq!(result, expected["result"]);
    reset();
}

#[test]
fn raw_parsed_html_without_interface_metadata_keeps_name_getters_exception_free() {
    tests::init_v8();
    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope,isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    install_with_html(scope, "<!doctype html><html><body></body></html>");
    let doc = parse_html_document(
        scope,
        "<!doctype html><html><body><div id=x data-value=v>text</div></body></html>",
        false,
    );
    let name = v8::String::new(scope, "rawParsedDocument").unwrap();
    context.global(scope).set(scope, name.into(), doc.into());
    v8::tc_scope!(let scope,scope);
    let source=v8::String::new(scope,"(()=>{const doc=rawParsedDocument,node=doc.getElementById('x'),attribute=node.getAttributeNode('data-value');return doc.documentElement.nodeName==='HTML'&&node.tagName==='DIV'&&attribute.nodeName==='data-value'&&node.firstChild.nodeName==='#text';})()").unwrap();
    let result = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
    assert!(
        result.is_some_and(|value| value.is_true()),
        "raw parsed record name read failed: {:?}",
        scope
            .exception()
            .map(|error| error.to_rust_string_lossy(scope))
    );
    reset();
}

#[test]
fn xml_element_case_namespaces_and_receivers_match_authored_chrome155_control() {
    tests::init_v8();
    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope,isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    install_with_html(
        scope,
        "<!doctype html><html><head></head><body></body></html>",
    );
    let probe = include_str!("xml_factory_elements_probe.js");
    let source = format!("JSON.stringify({})", probe.trim().trim_end_matches(';'));
    v8::tc_scope!(let scope,scope);
    let source = v8::String::new(scope, &source).unwrap();
    let result = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
    let result = result.unwrap_or_else(|| {
        panic!(
            "factory probe failed: {:?}",
            scope
                .exception()
                .map(|error| error.to_rust_string_lossy(scope))
        )
    });
    let result: serde_json::Value =
        serde_json::from_str(&result.to_rust_string_lossy(scope)).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("xml_factory_elements_chrome155.json")).unwrap();
    assert_eq!(result, expected["result"]);
    reset();
}
