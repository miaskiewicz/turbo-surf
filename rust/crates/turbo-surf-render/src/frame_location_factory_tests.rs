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

async fn component_page() -> PageSession {
    PageSession::open(
        "<body><iframe srcdoc='<body>child'></iframe></body>",
        "https://factory.test:8443/path?old=1#kept",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

fn pending_count(page: &PageSession) -> usize {
    page.rt
        .op_state()
        .borrow()
        .borrow::<auxiliary_pages::Pages>()
        .pending_count()
}

#[tokio::test(flavor = "current_thread")]
async fn protected_factory_components_normalize_without_spurious_invalid_navigation() {
    let mut page = component_page().await;
    check(
        &mut page,
        r#"(function(own) {
      const expected={origin:'https://factory.test:8443',protocol:'https:',host:'factory.test:8443',hostname:'factory.test',port:'8443',pathname:'/path',search:'?old=1',hash:'#kept'};
      if (!Object.entries(expected).every(([name,value])=>own[name]===value)) return false;
      const original=own.href;
      for (const [name,value] of [['protocol','1invalid:'],['hostname','bad host'],['host','['],['port','65536'],['port','invalid']]) {
        own[name]=value;if(own.href!==original)return false;
      }
      const token={};
      try { own.pathname={toString(){throw token}};return false; } catch(error) { if(error!==token)return false; }
      try { own.search=Symbol();return false; } catch(error) { if(error.name!=='TypeError')return false; }
      return Reflect.set(own,'origin','https://other.test')===false && own.href===original;
    })"#,
    );
    assert_eq!(pending_count(&page), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn protected_factory_component_setters_admit_normalized_native_navigation() {
    let cases = [
        (
            "protocol",
            "http:",
            "http://factory.test:8443/path?old=1#kept",
        ),
        (
            "hostname",
            "OTHER.TEST",
            "https://other.test:8443/path?old=1#kept",
        ),
        (
            "host",
            "other.test:9443",
            "https://other.test:9443/path?old=1#kept",
        ),
        ("port", "", "https://factory.test/path?old=1#kept"),
        ("port", "443", "https://factory.test/path?old=1#kept"),
        (
            "pathname",
            "/one two",
            "https://factory.test:8443/one%20two?old=1#kept",
        ),
        (
            "search",
            "?q=one two",
            "https://factory.test:8443/path?q=one%20two#kept",
        ),
        ("search", "", "https://factory.test:8443/path#kept"),
    ];
    for (name, value, expected) in cases {
        let mut page = component_page().await;
        let source = format!("(function(own){{own[{name:?}]={value:?};return true;}})");
        check(&mut page, &source);
        assert_eq!(pending_count(&page), 1, "{name}={value}");
        let request =
            auxiliary_pages::next_navigation(&mut page.rt.op_state().borrow_mut()).unwrap();
        assert_eq!(request.url, expected, "{name}={value}");
        assert_eq!(request.target.0, page.replaced_root_target().unwrap());
        assert!(!request.replace);
        assert!(request.history_mode.is_none());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn protected_factory_navigation_uses_actual_document_base_and_reload_mode() {
    let mut page = component_page().await;
    check(
        &mut page,
        r#"(function(own) {
      const base=document.createElement('base');base.href='/from/base/';document.head.append(base);
      own.replace('../next?x=1');return true;
    })"#,
    );
    let request = auxiliary_pages::next_navigation(&mut page.rt.op_state().borrow_mut()).unwrap();
    assert_eq!(request.url, "https://factory.test:8443/from/next?x=1");
    assert!(request.replace);
    assert!(request.history_mode.is_none());
    check(&mut page, "(function(own){own.reload();return true;})");
    let request = auxiliary_pages::next_navigation(&mut page.rt.op_state().borrow_mut()).unwrap();
    assert_eq!(request.url, "https://factory.test:8443/path?old=1#kept");
    assert!(request.replace);
    assert!(matches!(
        request.history_mode,
        Some(auxiliary_navigation::HistoryMode::Reload)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn protected_factory_invalid_navigation_preserves_conversion_exception_and_queue() {
    let mut page = component_page().await;
    check(
        &mut page,
        r#"(function(own) {
      const fail=operation=>{try{operation();return ''}catch(error){return error.name}};
      const token={};
      try{own.href={toString(){throw token}};return false}catch(error){if(error!==token)return false}
      return fail(()=>own.assign())==='TypeError'
        && fail(()=>own.replace())==='TypeError'
        && fail(()=>own.assign('http://['))==='SyntaxError'
        && fail(()=>own.replace('javascript:1'))==='NotSupportedError'
        && fail(()=>own.assign('data:text/html,blocked'))==='NotSupportedError';
    })"#,
    );
    assert_eq!(pending_count(&page), 0);
}

fn check_from_child(page: &mut PageSession, source: &str) {
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
        .expect("real initialized iframe");
    let target = window_proxy::protected_location(scope, root).unwrap();
    let scope = &mut v8::ContextScope::new(scope, child);
    let source = v8::String::new(scope, source).unwrap();
    let value = v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap();
    let function = v8::Local::<v8::Function>::try_from(value).unwrap();
    let undefined = v8::undefined(scope);
    assert!(function
        .call(scope, undefined.into(), &[target.into()])
        .is_some_and(|value| value.is_true()));
}

#[tokio::test(flavor = "current_thread")]
async fn protected_factory_top_navigation_obeys_real_sandbox_permissions() {
    for allowed in [false, true] {
        let permission = if allowed { " allow-top-navigation" } else { "" };
        let mut page=PageSession::open(
            &format!("<body><iframe sandbox='allow-scripts{permission}' srcdoc='<body>child'></iframe></body>"),
            "https://factory.test/root","","",5000,
        ).await.unwrap();
        let source = if allowed {
            "(function(root){root.href='/permitted';return true;})"
        } else {
            "(function(root){try{root.href='/blocked';return false}catch(error){return error.name==='SecurityError'}})"
        };
        check_from_child(&mut page, source);
        assert_eq!(pending_count(&page), usize::from(allowed));
        if allowed {
            let request =
                auxiliary_pages::next_navigation(&mut page.rt.op_state().borrow_mut()).unwrap();
            assert_eq!(request.url, "https://factory.test/permitted");
            assert_eq!(request.target.0, page.replaced_root_target().unwrap());
        }
    }
}

struct FactoryWireServer {
    origin: String,
    requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FactoryWireServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn factory_wire_server() -> FactoryWireServer {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let journal = requests.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut bytes = Vec::new();
            while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0, "request closed before headers");
                bytes.extend_from_slice(&chunk[..count]);
                assert!(bytes.len() < 65_536);
            }
            let request = String::from_utf8(bytes).unwrap();
            let target = request
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            journal.lock().unwrap().push(target.to_owned());
            let body = if target == "/root-barrier" {
                "root-progress"
            } else {
                "<!doctype html><title>physical-frame</title><p>loaded</p>"
            };
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    FactoryWireServer {
        origin,
        requests,
        task,
    }
}

fn check_current_factory(page: &mut PageSession, expected_url: &str, source: &str) -> u64 {
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
                && !native_frame_is_retired(*context)
                && window_proxy::location_url(*context).as_deref() == Some(expected_url)
        })
        .expect("current genuine frame document at the observed loaded URL");
    let document = child
        .get_slot::<ActiveDocument>()
        .unwrap()
        .0
        .to_local(scope)
        .unwrap();
    assert_eq!(document.get_creation_context(scope), Some(child));
    let document_id = turbo_dom_browser_env::host_internals::document_id_of(scope, document);
    let own = window_proxy::protected_location(scope, root).unwrap();
    let target = window_proxy::protected_location(scope, child).unwrap();
    assert_eq!(target.get_creation_context(scope), Some(child));
    let scope = &mut v8::ContextScope::new(scope, root);
    let source = v8::String::new(scope, source).unwrap();
    let value = v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap();
    let function = v8::Local::<v8::Function>::try_from(value).unwrap();
    let undefined = v8::undefined(scope);
    assert!(function
        .call(scope, undefined.into(), &[own.into(), target.into()])
        .is_some_and(|value| value.is_true()));
    document_id
}

#[tokio::test(flavor = "current_thread")]
async fn retired_protected_factory_cannot_fetch_after_a_live_frame_and_root_progress_barrier() {
    let server = factory_wire_server().await;
    let root_url = format!("{}/start", server.origin);
    let mut page = PageSession::open(
        "<body><iframe srcdoc='<body>initial'></iframe></body>",
        &root_url,
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let initial_document = check_current_factory(
        &mut page,
        "about:srcdoc",
        r#"(function(own, child) {
      const frame=document.querySelector('iframe');
      globalThis.factoryFrameLoad=new Promise(resolve=>frame.addEventListener('load',()=>resolve(frame.contentDocument.title),{once:true}));
      child.href='/frame-live';
      return true;
    })"#,
    );
    let loaded = page.eval_json_expression("factoryFrameLoad").await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&loaded).unwrap(),
        "physical-frame"
    );
    assert_eq!(*server.requests.lock().unwrap(), ["/frame-live"]);
    let live_document = check_current_factory(
        &mut page,
        &format!("{}/frame-live", server.origin),
        r#"(function(own, child) {
      const frame=document.querySelector('iframe');
      globalThis.removedFactoryWindow=frame.contentWindow;
      frame.remove();
      if(!removedFactoryWindow.closed)return false;
      child.href='/retired-href';
      child.replace('/retired-replace');
      child.reload();
      return true;
    })"#,
    );
    assert_ne!(
        live_document, initial_document,
        "retired calls must use the NEW physically loaded document factory"
    );
    let progress = page
        .eval_json_expression(
            r#"(async()=>{
      const body=await (await fetch('/root-barrier')).text();
      await new Promise(resolve=>setTimeout(resolve,0));
      return {body,closed:removedFactoryWindow.closed,frames:frames.length,href:location.href};
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&progress).unwrap(),
        serde_json::json!({
            "body":"root-progress","closed":true,"frames":0,"href":root_url
        })
    );
    assert_eq!(
        *server.requests.lock().unwrap(),
        ["/frame-live", "/root-barrier"]
    );
}
