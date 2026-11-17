//! Exercise protected factory objects without replacing any document surface.
use super::*;

async fn page(opaque: bool) -> PageSession {
    let sandbox = if opaque {
        "sandbox='allow-scripts'"
    } else {
        ""
    };
    PageSession::open(
        &format!("<body><iframe {sandbox} srcdoc='<body>child'></iframe></body>"),
        "https://factory.test/root",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

fn check(page: &mut PageSession, source: &str) {
    let _activation = page.rt.activation_key().enter();
    let root = private_infrastructure::context(&mut page.rt).unwrap();
    let frames = page
        .rt
        .op_state()
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    deno_core::scope!(scope, &mut page.rt);
    let root = v8::Local::new(scope, root);
    let child = frames
        .iter()
        .map(|frame| v8::Local::new(scope, frame))
        .find(|context| {
            context.get_slot::<FrameParent>().is_some()
                && context.get_slot::<FrameRetired>().is_none()
        })
        .expect("real initialized iframe context");
    let own_location = window_proxy::protected_location(scope, root).expect("root factory object");
    let child_location =
        window_proxy::protected_location(scope, child).expect("child factory object");
    let scope = &mut v8::ContextScope::new(scope, root);
    let source = v8::String::new(scope, source).unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    let value = script.run(scope).unwrap();
    let function = v8::Local::<v8::Function>::try_from(value).unwrap();
    let undefined = v8::undefined(scope);
    let result = function.call(
        scope,
        undefined.into(),
        &[own_location.into(), child_location.into()],
    );
    assert!(
        result.is_some_and(|value| value.is_true()),
        "protected factory contract failed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn protected_factory_location_keeps_native_component_brands() {
    let mut page = page(false).await;
    check(
        &mut page,
        r#"(function(own, child) {
      const fail = operation => { try { operation(); return ''; } catch(error) { return error.name; } };
      if (own.href !== 'https://factory.test/root' || child.href !== 'about:srcdoc') return false;
      for (const name of ['href','protocol','host','hostname','port','pathname','search','hash','origin']) {
        const descriptor=Object.getOwnPropertyDescriptor(own,name);
        if (!descriptor || descriptor.configurable || !descriptor.enumerable) return false;
        if (fail(()=>descriptor.get.call({})) !== 'TypeError') return false;
        if (descriptor.set && fail(()=>descriptor.set.call({},'x')) !== 'TypeError') return false;
      }
      return fail(()=>own.assign.call({},'/next'))==='TypeError'
        && fail(()=>own.replace.call({},'/next'))==='TypeError'
        && fail(()=>own.reload.call({}))==='TypeError';
    })"#,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn protected_factory_opaque_location_descriptors_preserve_sop() {
    let mut page = page(true).await;
    check(
        &mut page,
        r#"(function(own, child) {
      const fail = operation => { try { operation(); return ''; } catch(error) { return error.name; } };
      const href=Object.getOwnPropertyDescriptor(child,'href');
      if (href.get!==undefined || typeof href.set!=='function' || href.enumerable || !href.configurable) return false;
      if (fail(()=>href.set.call({},'/next'))!=='TypeError') return false;
      const replace=Object.getOwnPropertyDescriptor(child,'replace');
      if (replace.value!==child.replace || replace.writable || replace.enumerable || !replace.configurable) return false;
      for (const key of ['then',Symbol.toStringTag,Symbol.hasInstance,Symbol.isConcatSpreadable]) {
        const descriptor=Object.getOwnPropertyDescriptor(child,key);
        if (!(key in child) || child[key]!==undefined || descriptor.value!==undefined
            || descriptor.writable || descriptor.enumerable || !descriptor.configurable) return false;
      }
      const keys=Reflect.ownKeys(child);
      if (!['href','replace','then'].every(key=>keys.includes(key))) return false;
      return fail(()=>child.href)==='SecurityError'
        && fail(()=>Object.getOwnPropertyDescriptor(child,'origin'))==='SecurityError'
        && fail(()=>('assign' in child))==='SecurityError'
        && fail(()=>Reflect.set(child,'then',1))==='SecurityError'
        && fail(()=>Reflect.set(child,Symbol('private'),1))==='SecurityError';
    })"#,
    );
}
