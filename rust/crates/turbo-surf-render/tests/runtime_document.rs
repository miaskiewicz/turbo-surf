//! Runtime document metadata must retain canonical native tree and realm ownership.
use turbo_surf_render::{render_page, run_with_dom, PageSession};

#[test]
fn processing_instruction_probe_matches_independent_chrome_oracle() {
    let source = include_str!("../../../../harness/detection/document-character-data-probe.js");
    let result = run_with_dom("<!doctype html><body>", source).unwrap();
    let actual: serde_json::Value = serde_json::from_str(&result).unwrap();
    let expected = serde_json::json!({
      "metadata":[["xmlEncoding",null],["xmlStandalone",false],["xmlVersion",null],["rootElement",null],["compatMode","CSS1Compat"]],
      "cdata":["NotSupportedError",9,true],
      "invalid":[["bad target",["InvalidCharacterError",5,true]],["1target",["InvalidCharacterError",5,true]],[":target",null],["xml",null],["é",null],["a:b",null],["a\u{301}",null],["lone-surrogate",["InvalidCharacterError",5,true]]],
      "terminator":["InvalidCharacterError",5,true],"receiver":["TypeError",null,false],
      "initial":[7,"test","test","a😀b","a😀b","a😀b",4,true,true,"[object ProcessingInstruction]"],
      "edit":["a😀!b","😀",["IndexSizeError",1,true]],
      "clone":[true,"test","a😀!b",true],"parent":[true,"","<?test a😀!b?>"],
      "import":["test","a😀!b",true],"adopt":[true,true,true,"test","a😀!b"],
      "metadataMutation":[true,"1.0",["NotSupportedError",9,true],"1.0"],
      "conversion":["",["TypeError",null,false],["TypeError",null,false]],
      "elementNodeValue":[null,0],"childMethods":[null,"beforeafter"]
    });
    assert_eq!(actual, expected);
}

#[test]
fn document_runtime_members_live_on_the_canonical_prototype() {
    let result = run_with_dom("<head><title> first title </title></head><body><main>old</main></body>", r#"(() => {
      const names=['URL','documentURI','cookie','readyState','title','referrer','createElement'];
      const descriptor=Object.getOwnPropertyDescriptor(Document.prototype,'title');
      const checks=names.map(name=>Object.hasOwn(Document.prototype,name)&&!Object.hasOwn(document,name));
      const invalid=names.map(name=>{try{const d=Object.getOwnPropertyDescriptor(Document.prototype,name);if(d.get)d.get.call({});else d.value.call({},'div');return false;}catch(e){return e instanceof TypeError;}});
      document.title='changed';document.querySelector('title').textContent=' live  title ';
      const created=Document.prototype.createElement.call(document,'section');document.body.appendChild(created);
      const body=document.body;body.innerHTML='<p>updated</p>';
      const base=document.createElement('base');base.setAttribute('href','https://example.test/first/');document.head.appendChild(base);
      const firstBase=body.baseURI;base.setAttribute('href','https://example.test/second/');
      const liveBase=firstBase==='https://example.test/first/'&&document.baseURI==='https://example.test/second/'&&body.baseURI===document.baseURI;
      return JSON.stringify({checks,invalid,title:document.title,created:created.ownerDocument===document,
        liveBase,live:document.body===body&&document.querySelector('p').textContent==='updated',
        chain:Object.getPrototypeOf(document)===HTMLDocument.prototype&&Object.getPrototypeOf(HTMLDocument.prototype)===Document.prototype,
        branded:Object.prototype.toString.call(document),ownTag:Object.hasOwn(document,Symbol.toStringTag),
        getter:[descriptor.get.name,descriptor.get.length,descriptor.enumerable,!Object.hasOwn(descriptor.get,'prototype')],
        method:[document.createElement.name,document.createElement.length,!Object.hasOwn(document.createElement,'prototype')]});
    })()"#).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value["checks"],
        serde_json::json!([true, true, true, true, true, true, true])
    );
    assert_eq!(
        value["invalid"],
        serde_json::json!([true, true, true, true, true, true, true])
    );
    assert_eq!(value["title"], "live title");
    assert_eq!(value["branded"], "[object HTMLDocument]");
    assert_eq!(value["ownTag"], false);
    assert_eq!(
        value["getter"],
        serde_json::json!(["get title", 0, true, true])
    );
    assert_eq!(
        value["method"],
        serde_json::json!(["createElement", 1, true])
    );
    for name in ["created", "live", "chain", "liveBase"] {
        assert_eq!(value[name], true, "{value}");
    }
}

#[test]
fn reinstalling_a_document_keeps_metadata_and_native_callbacks_live() {
    for title in ["first", "second", "third"] {
        let html = format!("<head><title>{title}</title></head><body><p>{title}</p></body>");
        let result = run_with_dom(&html, r#"JSON.stringify({title:document.title,body:document.body.textContent,head:document.head.querySelector('title').textContent,own:Object.hasOwn(document,'title'),owner:Object.hasOwn(Document.prototype,'title')})"#).unwrap();
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["title"], title);
        assert_eq!(value["body"], title);
        assert_eq!(value["head"], title);
        assert_eq!(value["own"], false);
        assert_eq!(value["owner"], true);
    }
}

#[tokio::test]
async fn borrowed_document_members_target_the_actual_same_origin_frame() {
    let mut page = PageSession::open(
        "<head><title>parent</title></head><body><iframe id='child' srcdoc='<title>child</title><p>frame body</p>'></iframe></body>",
        "http://localhost/parent", "", "", 5000,
    ).await.unwrap();
    let result = page.eval(r#"globalThis.__RESULT = (() => {
      const frame=document.getElementById('child'),child=frame.contentDocument;
      const title=Object.getOwnPropertyDescriptor(Document.prototype,'title');
      const url=Object.getOwnPropertyDescriptor(Document.prototype,'URL');
      title.set.call(child,'borrowed child');
      const created=Document.prototype.createElement.call(child,'section');child.body.appendChild(created);
      const ready=Object.getOwnPropertyDescriptor(Document.prototype,'readyState').get.call(child);
      Object.defineProperty(child,'defaultView',{value:window,configurable:true});
      const retainedState=url.get.call(child)==='about:srcdoc'&&Object.getOwnPropertyDescriptor(Document.prototype,'currentScript').get.call(child)===null;delete child.defaultView;
      const cookie=Object.getOwnPropertyDescriptor(Document.prototype,'cookie');cookie.set.call(child,'frame=value; Path=/');
      return JSON.stringify({parent:document.title,parentUrl:document.URL,parentLocation:location.href,child:child.title,url:url.get.call(child),
        ready,retainedState,owner:created.ownerDocument===child,body:child.body.querySelector('p').textContent,
        cookie:cookie.get.call(child).includes('frame=value')&&document.cookie.includes('frame=value'),
        referrer:Object.getOwnPropertyDescriptor(Document.prototype,'referrer').get.call(child),
        chain:Object.getPrototypeOf(child)===frame.contentWindow.HTMLDocument.prototype,
        inherited:!Object.hasOwn(child,'URL')&&!Object.hasOwn(child,'cookie')&&!Object.hasOwn(child,'createElement')});
    })()"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["parent"], "parent");
    assert_eq!(value["child"], "borrowed child");
    assert_eq!(value["url"], "about:srcdoc");
    assert_eq!(value["ready"], "complete");
    assert_eq!(value["body"], "frame body");
    // The independent CSP navigation Chrome control selects origin-only for srcdoc.
    assert_eq!(value["referrer"], "http://localhost/", "{value}");
    for name in ["owner", "chain", "inherited", "cookie", "retainedState"] {
        assert_eq!(value[name], true, "{value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_cookie_getter_uses_the_frame_request_path() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (received, request) = tokio::sync::oneshot::channel();
    let received = std::sync::Arc::new(std::sync::Mutex::new(Some(received)));
    let response = tokio::spawn(async move {
        while let Ok((mut socket, _)) = server.accept().await {
            let received = received.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let count = socket.read(&mut bytes).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&bytes[..count]);
                    if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        break;
                    }
                }
                let html = "<head><title>remote child</title></head><body>frame</body>";
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len()).as_bytes()).await.unwrap();
                if let Some(received) = received.lock().unwrap().take() {
                    let _ = received.send(request);
                }
            });
        }
    });
    let base = format!("http://127.0.0.1:{}/parent", address.port());
    let mut cookies = turbo_surf_core::cookies::CookieJar::new();
    cookies.add("child", "only", "127.0.0.1", "/frame", None);
    let mut page = PageSession::open(
        "<body><iframe id='child' src='/frame/page'></iframe></body>",
        &base,
        &cookies.storage_state(),
        "",
        5000,
    )
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), request)
        .await
        .expect("localhost frame request was not made")
        .unwrap();
    response.abort();
    let result = page.eval(r#"globalThis.__RESULT = (() => {const child=document.getElementById('child').contentDocument;const cookie=Object.getOwnPropertyDescriptor(Document.prototype,'cookie');return JSON.stringify({parent:cookie.get.call(document),child:cookie.get.call(child),url:Object.getOwnPropertyDescriptor(Document.prototype,'URL').get.call(child)});})()"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["parent"], "");
    assert_eq!(value["child"], "child=only");
    assert_eq!(
        value["url"],
        format!("http://127.0.0.1:{}/frame/page", address.port())
    );
}

#[test]
fn runtime_document_state_is_private_and_receiver_branded() {
    let result=run_with_dom("<!doctype html><head></head><body></body>",r#"(() => {
      const names=['fonts','currentScript','readyState','scrollingElement','characterSet','charset','inputEncoding','onreadystatechange'];
      const checks=names.map(name=>{const descriptor=Object.getOwnPropertyDescriptor(Document.prototype,name);let invalid=false;try{descriptor.get.call({});}catch(error){invalid=error instanceof TypeError;}return [!Object.hasOwn(document,name),descriptor.enumerable,descriptor.configurable,invalid];});
      document.currentScript='fake';document[Symbol.for('__ts_rs')]='fake';
      const callback=()=>{};const handler=Object.getOwnPropertyDescriptor(Document.prototype,'onreadystatechange');handler.set.call(document,callback);
      const set=handler.get.call(document)===callback;handler.set.call(document,42);
      return JSON.stringify({checks,set,handler:document.onreadystatechange,currentScript:document.currentScript,readyState:document.readyState,encoding:document.characterSet,scrolling:document.scrollingElement===document.documentElement});
    })()"#).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value["checks"],
        serde_json::json!(vec![[true, true, true, true]; 8])
    );
    assert_eq!(value["currentScript"], serde_json::Value::Null);
    assert_eq!(value["handler"], serde_json::Value::Null);
    assert_eq!(value["set"], true);
    assert_eq!(value["readyState"], "complete");
    assert_eq!(value["encoding"], "UTF-8");
    assert_eq!(value["scrolling"], true);
}

#[test]
fn document_compatibility_members_are_inherited_and_body_is_live() {
    let result=run_with_dom("<!doctype html><head><title>live</title><style>p {color:red}</style></head><body><p>old</p></body>",r#"(() => {
      const names=['body','head','documentElement','defaultView','styleSheets','adoptedStyleSheets','createRange','createTreeWalker','hasFocus','contentType'];
      const checks=names.map(name=>{const descriptor=Object.getOwnPropertyDescriptor(Document.prototype,name);let invalid=false;try{if(descriptor.get)descriptor.get.call({});else descriptor.value.call({});}catch(error){invalid=error instanceof TypeError;}return [!!descriptor,!Object.hasOwn(document,name),invalid];});
      const before=document.body,newBody=document.createElement('body');newBody.innerHTML='<section>new</section>';document.body=newBody;
      let rejected=false;try{document.body=document.createElement('div');}catch(error){rejected=error.name==='HierarchyRequestError';}
      const bodyErrors=[{},document.createElementNS('http://www.w3.org/2000/svg','body')].map(value=>{try{document.body=value;return 'accepted';}catch(error){return error.name;}});
      const events=[];document.onclick=()=>events.push('first');document.addEventListener('click',()=>events.push('listener'));document.onclick=()=>events.push('replacement');document.dispatchEvent(new Event('click'));document.onclick=null;document.onclick=()=>events.push('last');document.dispatchEvent(new Event('click'));
      return JSON.stringify({checks,own:Reflect.ownKeys(document).map(String).sort(),body:document.body===newBody&&document.querySelector('section').textContent==='new'&&!before.isConnected,head:document.head.querySelector('title').textContent==='live',root:document.documentElement.localName==='html',view:document.defaultView===window,rejected,bodyErrors,events});
    })()"#).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value["checks"],
        serde_json::json!(vec![[true, true, true]; 10])
    );
    assert_eq!(value["own"], serde_json::json!(["location"]));
    assert_eq!(
        value["bodyErrors"],
        serde_json::json!(["TypeError", "HierarchyRequestError"])
    );
    assert_eq!(
        value["events"],
        serde_json::json!(["replacement", "listener", "listener", "last"])
    );
    for key in ["body", "head", "root", "view", "rejected"] {
        assert_eq!(value[key], true, "{value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn retired_iframe_document_has_no_default_view_and_keeps_its_own_metadata() {
    let mut page = PageSession::open(
        "<html><body><iframe id='f' srcdoc='<p>old</p>'></iframe></body></html>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval("globalThis.oldFrameDocument=document.getElementById('f').contentDocument;globalThis.oldFrameWindow=document.getElementById('f').contentWindow;globalThis.oldFrameURL=oldFrameDocument.URL;document.getElementById('f').srcdoc='<p>new</p>';").await.unwrap();
    let result = page.eval("globalThis.__RESULT=JSON.stringify({retired:oldFrameDocument.defaultView===null,borrowed:Object.getOwnPropertyDescriptor(Document.prototype,'defaultView').get.call(oldFrameDocument)===null,url:oldFrameDocument.URL===oldFrameURL,current:document.getElementById('f').contentDocument.defaultView===document.getElementById('f').contentWindow,kept:document.getElementById('f').contentWindow===oldFrameWindow,fresh:document.getElementById('f').contentDocument!==oldFrameDocument})").await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"retired":true,"borrowed":true,"url":true,"current":true,"kept":cfg!(turbo_surf_native_proxy),"fresh":true})
    );
}

// getSelection is the binding's own Document.prototype member over per-document state,
// not a creation-context delegate: like Chrome, a document without a browsing context
// (retired by navigation, or parsed) has no selection.
#[test]
fn get_selection_is_inherited_native_and_per_document() {
    let result = run_with_dom("<!doctype html><body><p id=a>hello</p></body>", r#"(() => {
      const descriptor=Object.getOwnPropertyDescriptor(Document.prototype,'getSelection');
      let invalid=false;try{descriptor.value.call({});}catch(error){invalid=error instanceof TypeError;}
      const selection=document.getSelection();selection.selectAllChildren(document.getElementById('a'));
      return JSON.stringify({own:Object.hasOwn(document,'getSelection'),native:Function.prototype.toString.call(descriptor.value)==='function getSelection() { [native code] }',
        invalid,same:selection===getSelection(),brand:selection instanceof Selection,text:String(selection),
        parsed:new DOMParser().parseFromString('<p>x</p>','text/html').getSelection()});
    })()"#).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"own":false,"native":true,"invalid":true,"same":true,"brand":true,"text":"hello","parsed":null})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retired_iframe_document_has_no_selection() {
    let mut page = PageSession::open(
        "<html><body><iframe id='f' srcdoc='<p id=q>old</p>'></iframe></body></html>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval("globalThis.oldFrameDocument=document.getElementById('f').contentDocument;globalThis.oldSelection=oldFrameDocument.getSelection();oldSelection.selectAllChildren(oldFrameDocument.getElementById('q'));globalThis.before=[String(oldSelection),oldSelection===document.getElementById('f').contentWindow.getSelection(),oldSelection!==document.getSelection()];document.getElementById('f').srcdoc='<p>new</p>';").await.unwrap();
    let result = page.eval("globalThis.__RESULT=JSON.stringify({before,retired:oldFrameDocument.getSelection(),borrowed:Document.prototype.getSelection.call(oldFrameDocument),fresh:document.getElementById('f').contentDocument.getSelection()===document.getElementById('f').contentWindow.getSelection()})").await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"before":["old",true,true],"retired":null,"borrowed":null,"fresh":true})
    );
}

#[tokio::test]
async fn adopted_iframe_retires_its_context_and_reinsertion_gets_a_fresh_document() {
    let html = render_page(
        "<!doctype html><body><p id='result'></p></body>",
        "https://example.test/page",
        r#"const frame=document.createElement('iframe');document.body.append(frame);
        const old=frame.contentDocument, window=frame.contentWindow;
        const detached=new DOMParser().parseFromString('<!doctype html><body></body>','text/html');
        const same=detached.adoptNode(frame)===frame;
        detached.body.append(frame);
        const inactive=frame.contentWindow===null&&old.defaultView===null&&frames.length===0;
        document.adoptNode(frame);document.body.append(frame);
        const fresh=frame.contentDocument!==old&&frame.contentWindow!==null&&frames.length===1;
        document.getElementById('result').textContent=[same,inactive,fresh,frame.ownerDocument===document].join(',');"#,
    ).await.unwrap();
    assert!(html.contains("true,true,true,true"), "{html}");
}

#[test]
fn repeated_evaluations_share_one_document_but_changed_html_gets_fresh_intrinsics_and_lexicals() {
    let first = "<!doctype html><body><p id='state'>first</p></body>";
    assert_eq!(run_with_dom(first, r#"let persistentCount=1;const lexicalSecret='first';globalThis.pageSecret='first';Array.prototype.pageMarker='mutated';Object.prototype.objectMarker='mutated';document.getElementById('state').textContent='retained';globalThis.keptDocument=document;'initialized'"#).unwrap(),"initialized");
    assert_eq!(run_with_dom(first, r#"persistentCount++;JSON.stringify([persistentCount,lexicalSecret,pageSecret,Array.prototype.pageMarker,document.getElementById('state').textContent,document===keptDocument])"#).unwrap(),r#"[2,"first","first","mutated","retained",true]"#);
    let second = "<!doctype html><body><p id='state'>second</p></body>";
    assert_eq!(run_with_dom(second,r#"JSON.stringify([typeof persistentCount,typeof lexicalSecret,typeof pageSecret,typeof Array.prototype.pageMarker,typeof Object.prototype.objectMarker,typeof keptDocument,document.getElementById('state').textContent])"#).unwrap(),r#"["undefined","undefined","undefined","undefined","undefined","undefined","second"]"#);
    assert_eq!(run_with_dom(second,"let persistentCount=7;const lexicalSecret='second';String(persistentCount)+':'+lexicalSecret").unwrap(),"7:second");
    assert_eq!(run_with_dom(first,"let persistentCount=9;const lexicalSecret='fresh-first';String(persistentCount)+':'+lexicalSecret+':'+document.getElementById('state').textContent").unwrap(),"9:fresh-first:first");
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_document_aliases_read_private_state_despite_own_property_overrides() {
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><body></body>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    assert_eq!(page.eval(r#"
      const actual=[document.hidden,document.visibilityState,document.fullscreenElement];
      for(const name of ['hidden','visibilityState','fullscreenElement'])Object.defineProperty(document,name,{get(){throw new Error('guest shadow getter must not supply alias state')}});
      const values=[document.webkitHidden,document.webkitVisibilityState,document.webkitFullscreenElement,document.webkitCurrentFullScreenElement,document.webkitIsFullScreen];
      const getter=Object.getOwnPropertyDescriptor(Document.prototype,'webkitHidden').get;
      let error;try{getter.call({})}catch(e){error=e.name}
      globalThis.__RESULT=JSON.stringify([values,[actual[0],actual[1],actual[2],actual[2],actual[2]!==null],error]);
    "#).await.unwrap(), r#"[[false,"visible",null,null,false],[false,"visible",null,null,false],"TypeError"]"#);
}
