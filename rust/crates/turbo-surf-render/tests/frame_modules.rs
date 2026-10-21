//! Genuine child-realm module maps must not execute against the main document.
#![cfg(turbo_surf_native_proxy)]
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn child_module_graphs_run_in_their_own_realms_and_keep_separate_caches() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = requests.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let requests = observed.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                requests.lock().unwrap().push(path.clone());
                let (mime, body) = match path.as_str() {
                    "/modules/frame-child.html" => (
                        "text/html",
                        include_str!(
                            "../../../../harness/detection/fixtures/modules/frame-child.html"
                        ),
                    ),
                    "/modules/frame-entry.mjs" => (
                        "application/javascript",
                        include_str!(
                            "../../../../harness/detection/fixtures/modules/frame-entry.mjs"
                        ),
                    ),
                    "/modules/frame-dep.mjs" => (
                        "application/javascript",
                        include_str!(
                            "../../../../harness/detection/fixtures/modules/frame-dep.mjs"
                        ),
                    ),
                    _ => ("text/plain", "missing"),
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let base = format!("http://{address}/parent");
    let html = include_str!("../../../../harness/detection/fixtures/frame-modules.html");
    let mut page = PageSession::open(html, &base, "", "", 10000).await.unwrap();
    let result=page.eval(r#"globalThis.__RESULT=JSON.stringify({parent:document.title,unpolluted:globalThis.entryRuns===undefined,frames:['a','b'].map(id=>{const win=document.getElementById(id).contentWindow;return {title:win.document.title,result:win.moduleResult,inline:win.inlineModule,depRuns:win.depRuns,entryRuns:win.entryRuns,events:win.frameLog,ownArray:win.Array!==Array};})})"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    let fixture = page
        .eval("globalThis.__RESULT=JSON.stringify(__browserFixture)")
        .await
        .unwrap();
    let fixture: serde_json::Value = serde_json::from_str(&fixture).unwrap();
    assert_eq!(
        fixture["frames"].as_array().unwrap().len(),
        2,
        "parent Window load must observe child module completion: {fixture}"
    );
    assert_eq!(value["parent"], "parent", "{value}");
    assert_eq!(value["unpolluted"], true, "{value}");
    for frame in value["frames"].as_array().unwrap() {
        assert_eq!(frame["title"], "module child", "{value}");
        assert_eq!(frame["result"]["owned"], "child", "{value}");
        assert_eq!(frame["result"]["current"], true, "{value}");
        assert_eq!(frame["inline"]["current"], true, "{value}");
        assert_eq!(frame["inline"]["owned"], "child", "{value}");
        assert!(frame["inline"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/modules/frame-child.html"));
        assert!(frame["result"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/modules/frame-entry.mjs"));
        assert_eq!(frame["depRuns"], 1, "{value}");
        assert_eq!(frame["entryRuns"], 1, "{value}");
        assert_eq!(
            frame["events"],
            serde_json::json!(["module", "dcl", "load"]),
            "{value}"
        );
        assert_eq!(frame["ownArray"], true, "{value}");
    }
    let requests = requests.lock().unwrap().clone();
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.as_str() == "/modules/frame-dep.mjs")
            .count(),
        2,
        "{requests:?}"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.as_str() == "/modules/frame-entry.mjs")
            .count(),
        2,
        "{requests:?}"
    );
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn child_async_graphs_and_top_level_await_release_lifecycle_and_report_real_errors() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let dcl = std::sync::Arc::new(tokio::sync::Notify::new());
    let load = std::sync::Arc::new(tokio::sync::Notify::new());
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let dcl = dcl.clone();
            let load = load.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (mime,body)=match path{
            "/child"=>("text/html",r#"<head><script>
              globalThis.events=[];globalThis.errors=[];
              document.addEventListener('DOMContentLoaded',()=>{events.push('dcl');fetch('/dcl-gate');});
              window.addEventListener('load',()=>{events.push('load');fetch('/load-gate');});
              window.addEventListener('error',event=>errors.push({name:event.error.name,trusted:event.isTrusted,owned:event.target===window,source:event.filename,line:event.lineno}));
              </script><script id='tla' type='module' src='/tla.mjs'></script><script id='asynchronous' type='module' async src='/async.mjs'></script><script id='failure' type='module' src='/error.mjs'></script><script id='mime' type='module' src='/mime.mjs'></script>
              <script>for(const id of ['tla','asynchronous','failure','mime']){const el=document.getElementById(id);el.addEventListener('load',()=>events.push(id+':load'));el.addEventListener('error',()=>events.push(id+':error'));}</script></head><body></body>"#),
            "/async.mjs"=>{dcl.notified().await;("application/javascript","globalThis.asyncReady=document.currentScript===null;events.push('async:eval');")},
            "/tla.mjs"=>("application/javascript","globalThis.tlaStarted=true;events.push('tla:start');await fetch('/tla-wait');globalThis.tlaDone=true;events.push('tla:end');"),
            "/error.mjs"=>("application/javascript","throw new RangeError('native-child-module-error');"),
            "/mime.mjs"=>("text/plain","globalThis.wrongMimeExecuted=true;"),
            "/dcl-gate"=>{dcl.notify_one();("text/plain","ready")},
            "/load-gate"=>{load.notify_one();("text/plain","ready")},
            "/tla-wait"=>{load.notified().await;("text/plain","ready")},
            _=>("text/plain","missing"),
          };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let mut page = PageSession::open(
        "<body><iframe id='child' src='/child'></iframe></body>",
        &format!("http://{address}/parent"),
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let result=page.eval(r#"globalThis.__RESULT=JSON.stringify((()=>{const frame=document.getElementById('child').contentWindow;return {events:frame.events,errors:frame.errors,asyncReady:frame.asyncReady,tlaStarted:frame.tlaStarted,tlaDone:frame.tlaDone,wrongMime:frame.wrongMimeExecuted===undefined};})())"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    for key in ["asyncReady", "tlaStarted", "tlaDone", "wrongMime"] {
        assert_eq!(value[key], true, "{value}");
    }
    let events = value["events"].as_array().unwrap();
    let position = |name| events.iter().position(|item| item == name).unwrap();
    assert!(position("tla:start") < position("tla:load"), "{value}");
    assert!(position("tla:load") < position("dcl"), "{value}");
    assert!(position("dcl") < position("async:eval"), "{value}");
    assert!(position("asynchronous:load") < position("load"), "{value}");
    assert!(position("load") < position("tla:end"), "{value}");
    assert_eq!(
        events.iter().filter(|item| *item == "mime:error").count(),
        1,
        "{value}"
    );
    assert_eq!(
        events.iter().filter(|item| *item == "failure:load").count(),
        1,
        "{value}"
    );
    let errors = value["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "{value}");
    assert_eq!(errors[0]["name"], "RangeError");
    assert_eq!(errors[0]["trusted"], true);
    assert_eq!(errors[0]["owned"], true);
    assert_eq!(errors[0]["line"], 1);
    assert!(errors[0]["source"]
        .as_str()
        .unwrap()
        .ends_with("/error.mjs"));
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn navigation_cancels_the_retired_child_graph_without_reusing_its_module_map() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let started = started.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (mime,body)=match path {
            "/old"=>("text/html","<title>old</title><script type='module' src='/slow.mjs'></script><body>old</body>"),
            "/slow.mjs"=>{started.notify_one();std::future::pending::<()>().await;unreachable!()},
            "/await-request"=>{started.notified().await;("text/plain","queued")},
            "/new"=>("text/html","<title>new</title><script type='module' src='/new.mjs'></script><body>new</body>"),
            "/new.mjs"=>("application/javascript","globalThis.replacementModule=true;document.title='replacement';"),
            _=>("text/plain","missing"),
          };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let html = r#"<body><iframe id='child' src='/old'></iframe><script>fetch('/await-request').then(()=>{const frame=document.getElementById('child');globalThis.oldFrameWindow=frame.contentWindow;globalThis.oldFrameDocument=frame.contentDocument;frame.src='/new';});</script></body>"#;
    let mut page = PageSession::open(html, &format!("http://{address}/parent"), "", "", 10000)
        .await
        .unwrap();
    let result=page.eval(r#"globalThis.__RESULT=JSON.stringify((()=>{const frame=document.getElementById('child');return {title:frame.contentDocument.title,replacement:frame.contentWindow.replacementModule,stable:frame.contentWindow===oldFrameWindow,oldView:oldFrameDocument.defaultView===null,oldTitle:oldFrameDocument.title};})())"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"title":"replacement","replacement":true,"stable":true,"oldView":true,"oldTitle":"old"})
    );
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn sandbox_module_fetch_uses_the_native_opaque_origin_and_scripts_blocked_frames_are_inert() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = requests.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let observed = observed.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                observed.lock().unwrap().push((path.clone(), request));
                let (mime,body)=match path.as_str(){
          "/opaque"=>("text/html","<script type='module' src='/opaque.mjs'></script><body></body>"),
          "/blocked"=>("text/html","<script type='module' src='/blocked.mjs'></script><body></body>"),
          "/opaque.mjs"=>("application/javascript","parent.postMessage({module:true,origin:origin,current:document.currentScript===null},'*');"),
          "/blocked.mjs"=>("application/javascript","globalThis.blockedModuleExecuted=true;"),
          _=>("text/plain","missing"),
        };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let html="<script>window.addEventListener('message',event=>{globalThis.moduleMessage={data:event.data,origin:event.origin};});</script><body><iframe id='opaque' sandbox='allow-scripts' src='/opaque'></iframe><iframe id='blocked' sandbox='allow-same-origin' src='/blocked'></iframe></body>";
    let base = format!("http://{address}/parent");
    let mut jar = turbo_surf_core::cookies::CookieJar::new();
    jar.set_from_document(&base, "secret=yes; Path=/", 0.0);
    assert!(jar.cookie_header(&base, 0.0).contains("secret=yes"));
    let mut page = PageSession::open(html, &base, &jar.storage_state(), "", 10000)
        .await
        .unwrap();
    let result=page.eval(r#"globalThis.__RESULT=JSON.stringify({message:moduleMessage,blocked:document.getElementById('blocked').contentWindow.blockedModuleExecuted===undefined})"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"message":{"data":{"module":true,"origin":"null","current":true},"origin":"null"},"blocked":true})
    );
    let requests = requests.lock().unwrap().clone();
    let module = requests
        .iter()
        .find(|(path, _)| path == "/opaque.mjs")
        .unwrap()
        .1
        .to_ascii_lowercase();
    assert!(module.contains("origin: null\r\n"), "{module}");
    assert!(!module.contains("cookie:"), "{module}");
    assert!(
        !requests.iter().any(|(path, _)| path == "/blocked.mjs"),
        "{requests:?}"
    );
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn redirected_child_module_uses_the_final_source_url_for_relative_imports() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                if path == "/entry.mjs" {
                    socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: /final/entry.mjs\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                    return;
                }
                let (mime, body) = match path {
                    "/frame" => ("text/html", "<body><script type='module' src='/entry.mjs'></script></body>"),
                    "/final/entry.mjs" => ("text/javascript", "import {dependency} from './dep.mjs'; globalThis.redirected={dependency,url:import.meta.url,current:document.currentScript===null};"),
                    "/final/dep.mjs" => ("text/javascript", "export const dependency=import.meta.url;"),
                    _ => ("text/plain", "not executable"),
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let mut page = PageSession::open(
        "<body><iframe src='/frame'></iframe></body>",
        &format!("http://{address}/parent"),
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let result = page.eval("globalThis.__RESULT=JSON.stringify(document.querySelector('iframe').contentWindow.redirected)").await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"dependency":format!("http://{address}/final/dep.mjs"),"url":format!("http://{address}/final/entry.mjs"),"current":true})
    );
    server.abort();
}
