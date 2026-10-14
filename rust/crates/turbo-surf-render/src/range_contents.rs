//! Private Range accessor installation; all tree operations use captured DOM methods.

struct RangeAccessor(v8::Weak<v8::Function>);
struct RangeExceptionUpdater(v8::Weak<v8::Function>);

pub(super) fn capture(scope: &mut v8::PinScope) {
    let global = scope.get_current_context().global(scope);
    let Some(name) = v8::String::new(scope, "__nativeRangeRecord") else {
        return;
    };
    let Some(function) = global
        .get(scope, name.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    else {
        return;
    };
    let Some(accessor) = super::dom_ownership::helper_function(scope, function) else {
        return;
    };
    scope
        .get_current_context()
        .set_slot(std::rc::Rc::new(RangeAccessor(accessor)));
}

pub(super) fn install(scope: &mut v8::PinScope) {
    let Some(accessor) = scope.get_current_context().get_slot::<RangeAccessor>() else {
        return;
    };
    let accessor = super::dom_ownership::local(scope, &accessor.0);
    let global = scope.get_current_context().global(scope);
    let Some(name) = v8::String::new(scope, "ts") else {
        return;
    };
    let key = v8::Symbol::for_key(scope, name);
    let Some(namespace) = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    else {
        return;
    };
    let Some(name) = v8::String::new(scope, "makeNativeFunction") else {
        return;
    };
    let Some(factory) = namespace.get(scope, name.into()) else {
        return;
    };
    let Some(source) = v8::String::new(scope, include_str!("range_contents.js")) else {
        return;
    };
    if let Some(initializer) = v8::Script::compile(scope, source, None)
        .and_then(|script| script.run(scope))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    {
        let marker = v8::Function::new(scope, super::script_state::contextual_fragment).unwrap();
        let receiver = v8::undefined(scope);
        if let Some(updater) = initializer
            .call(
                scope,
                receiver.into(),
                &[accessor.into(), factory, marker.into()],
            )
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        {
            let Some(updater) = super::dom_ownership::helper_function(scope, updater) else {
                return;
            };
            scope
                .get_current_context()
                .set_slot(std::rc::Rc::new(RangeExceptionUpdater(updater)));
        }
    }
}

pub(super) fn capture_interfaces(scope: &mut v8::PinScope) {
    let Some(updater) = scope
        .get_current_context()
        .get_slot::<RangeExceptionUpdater>()
    else {
        return;
    };
    let function = super::dom_ownership::local(scope, &updater.0);
    let global = scope.get_current_context().global(scope);
    let Some(name) = v8::String::new(scope, "DOMException") else {
        return;
    };
    if let Some(constructor) = global.get(scope, name.into()) {
        let receiver = v8::undefined(scope);
        function.call(scope, receiver.into(), &[constructor]);
    }
}

pub(super) fn clear_context(context: v8::Local<v8::Context>) {
    context.remove_slot::<RangeAccessor>();
    context.remove_slot::<RangeExceptionUpdater>();
}

#[cfg(test)]
mod tests {
    use super::super::{
        capture_window_event_interfaces, install_with_html, parse_html_document, reset, run_js,
    };

    fn check_script(source: &str, refresh: Option<&str>) {
        super::super::tests::init_v8();
        let isolate = &mut v8::Isolate::new(Default::default());
        v8::scope!(let scope,isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        install_with_html(scope, "<!doctype html><body></body>");
        if let Some(setup) = refresh {
            run_js(scope, setup);
            capture_window_event_interfaces(scope);
        }
        let source = v8::String::new(scope, source).unwrap();
        v8::tc_scope!(let scope,scope);
        let value = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
        assert!(
            value.is_some_and(|value| value.is_true()),
            "Range contract failed: {:?}",
            scope
                .exception()
                .map(|error| error.to_rust_string_lossy(scope))
        );
        reset();
    }

    #[test]
    fn range_errors_use_the_private_final_exception_constructor() {
        check_script(
            r#"(()=>{const range=document.createRange();let hooks=0;globalThis.DOMException=function(){hooks++;throw new Error('guest constructor')};let error;try{range.setStart(document.doctype,0)}catch(caught){error=caught}return hooks===0&&error instanceof FinalException&&error.name==='InvalidNodeTypeError'})()"#,
            Some("globalThis.FinalException=class FinalException extends DOMException{};globalThis.DOMException=FinalException;"),
        );
    }

    #[test]
    fn range_content_operations_bypass_replaced_dom_methods_and_own_getters() {
        check_script(
            r#"(()=>{
          const host=document.createElement('p');host.textContent='abcdef';document.body.append(host);
          const text=host.firstChild,range=document.createRange(),wrapper=document.createElement('mark');range.setStart(text,1);range.setEnd(text,4);
          let hooks=0;const poison=()=>{hooks++;throw new Error('guest DOM hook')};
          const saved=[];for(const [proto,names]of [[Node.prototype,['cloneNode','appendChild','removeChild','insertBefore']],[Text.prototype,['splitText','deleteData']]])for(const name of names){saved.push([proto,name,Object.getOwnPropertyDescriptor(proto,name)]);Object.defineProperty(proto,name,{value:poison,writable:true,configurable:true})}
          Object.defineProperty(text,'data',{get:poison,configurable:true});Object.defineProperty(text,'parentNode',{get:poison,configurable:true});
          let clone,wrapped;try{clone=range.cloneContents().textContent;range.surroundContents(wrapper);wrapped=wrapper.textContent}finally{delete text.data;delete text.parentNode;for(const [proto,name,descriptor]of saved){if(descriptor)Object.defineProperty(proto,name,descriptor);else delete proto[name]}}
          return hooks===0&&clone==='bcd'&&wrapped==='bcd'&&host.innerHTML==='a<mark>bcd</mark>ef';
        })()"#,
            None,
        );
    }

    #[test]
    fn invalid_range_insertion_does_not_split_text_before_hierarchy_validation() {
        check_script(
            r#"(()=>{
          const host=document.createElement('p');host.textContent='abcdef';document.body.append(host);const text=host.firstChild,range=document.createRange();range.setStart(text,2);range.collapse(true);
          let error;try{range.insertNode(document)}catch(caught){error=caught.name}
          const detached=document.createTextNode('detached'),other=document.createRange();other.selectNodeContents(detached);
          return error==='HierarchyRequestError'&&host.childNodes.length===1&&text.data==='abcdef'&&range.startContainer===text&&range.startOffset===2&&other.cloneContents().textContent==='detached';
        })()"#,
            None,
        );
    }

    #[test]
    fn range_contents_and_boundaries_match_authored_chrome155_control() {
        super::super::tests::init_v8();
        let isolate = &mut v8::Isolate::new(Default::default());
        v8::scope!(let scope,isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        super::super::install_with_html(scope, "<!doctype html><body></body>");
        // The standalone binding lacks DOMImplementation; inject a genuine
        // independently parsed document for the control's cross-tree queries.
        let foreign = parse_html_document(scope, "<!doctype html><body></body>", false);
        let key = v8::String::new(scope, "__rangeForeignDoc").unwrap();
        scope
            .get_current_context()
            .global(scope)
            .set(scope, key.into(), foreign.into());
        let probe = include_str!("range_probe.js").replace(
            "document.implementation.createHTMLDocument(\"foreign\")",
            "__rangeForeignDoc",
        );
        let source = format!("JSON.stringify({})", probe.trim().trim_end_matches(';'));
        let source = v8::String::new(scope, &source).unwrap();
        v8::tc_scope!(let scope,scope);
        let value = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
        let value = value.unwrap_or_else(|| {
            panic!(
                "Range control failed: {:?}",
                scope
                    .exception()
                    .map(|error| error.to_rust_string_lossy(scope))
            )
        });
        let actual: serde_json::Value =
            serde_json::from_str(&value.to_rust_string_lossy(scope)).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("range_chrome155.json")).unwrap();
        assert_eq!(actual, expected["result"]);
        reset();
    }
}
