use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::{render_page, PageSession};

async fn fixture_server() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut bytes = [0; 4096];
            let count = stream.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..count]);
            let frame = request.split_whitespace().nth(1) == Some("/frame");
            let (status, content, body) = if frame {
                ("200 OK", "text/html", "<p id='secret'>secret</p>")
            } else if request.split_whitespace().nth(1) == Some("/throw.mjs") {
                (
                    "200 OK",
                    "application/javascript",
                    "throw new Error('module-error')",
                )
            } else {
                ("404 Not Found", "text/plain", "missing")
            };
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: {content}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (port, task)
}

#[tokio::test]
async fn guest_namespace_cannot_mint_trusted_lifecycle_or_mouse_events() {
    let html = render_page("<!doctype html><body>", "https://example.test/", r#"
      const ns=globalThis[Symbol.for('ts')];
      const names=['frameParsingDone','frameDomReady','fireFrameLoad','deliverWindowMessage','markDocumentInteractive','dispatchDocumentInteractive','dispatchDocumentComplete','completeClassicScript','completeModuleGraph'];
      document.body.setAttribute('data-hidden',String(names.every(name=>ns[name]===undefined)));
      let clicks=0,loads=0;
      document.addEventListener('click',event=>{if(event.isTrusted)clicks++;});
      window.addEventListener('load',event=>{if(event.isTrusted)loads++;});
      Event=function(){return new MouseEvent('click',{bubbles:true});};
      ns.dispatchDocumentComplete=()=>{throw Error('guest lifecycle replacement must not run');};
      window.onload=function(){document.body.setAttribute('data-caller',String(window.onload.caller===null));};
      window.addEventListener('pageshow',()=>{document.body.setAttribute('data-trust',clicks+':'+loads);});
    "#).await.unwrap();
    assert!(html.contains("data-hidden=\"true\""), "{html}");
    assert!(html.contains("data-trust=\"0:1\""), "{html}");
    assert!(html.contains("data-caller=\"true\""), "{html}");
}

#[tokio::test]
async fn resources_use_captured_event_constructors_after_guest_replacement() {
    let (port, task) = fixture_server().await;
    let mut page = PageSession::open(
        "<!doctype html><body>",
        &format!("http://127.0.0.1:{port}/"),
        "",
        "",
        4000,
    )
    .await
    .unwrap();
    page.eval(r#"
      let clicks=0,resourceErrors=0,moduleErrors=0;
      document.addEventListener('click',event=>{if(event.isTrusted)clicks++;});
      window.addEventListener('click',event=>{if(event.isTrusted)clicks++;});
      window.addEventListener('error',event=>{if(event.isTrusted)moduleErrors++;});
      Event=ErrorEvent=function(){return new MouseEvent('click',{bubbles:true});};
      const link=document.createElement('link');link.rel='stylesheet';link.href='/missing.css';
      link.addEventListener('error',event=>{if(event.type==='error'&&event.isTrusted)resourceErrors++;});document.head.appendChild(link);
      const module=document.createElement('script');module.type='module';module.src='/throw.mjs';document.head.appendChild(module);

    "#).await.unwrap();
    let result = page
        .eval("globalThis.__RESULT=clicks+':'+resourceErrors+':'+moduleErrors")
        .await
        .unwrap();
    page.close();
    task.abort();
    // The published 150 fallback logs module exceptions without the 154 native
    // module-stage ErrorEvent bridge. Both paths must preserve event type/trust.
    let expected = if cfg!(turbo_surf_native_proxy) {
        "0:1:1"
    } else {
        "0:1:0"
    };
    assert_eq!(result, expected);
}

#[tokio::test]
async fn frame_records_do_not_expose_a_cross_origin_document() {
    let (port, task) = fixture_server().await;
    let script = format!(
        r#"
      const frame=document.createElement('iframe');frame.src='http://127.0.0.1:{port}/frame';
      frame.onload=()=>{{
        const namespace=globalThis[Symbol.for('ts')];
        let blocked=false;try{{void frame.contentWindow.document;}}catch(error){{blocked=error.name==='SecurityError';}}
        document.body.setAttribute('data-safe',String(blocked&&frame.contentDocument===null&&namespace.frameRecordFor===undefined));
      }};document.body.appendChild(frame);
    "#
    );
    let html = render_page(
        "<!doctype html><body>",
        &format!("http://localhost:{port}/"),
        &script,
    )
    .await
    .unwrap();
    task.abort();
    assert!(html.contains("data-safe=\"true\""), "{html}");
}

#[tokio::test]
async fn created_srcdoc_frame_runs_native_handler_after_resources_and_owns_its_tree() {
    let html = render_page("<!doctype html><body><div id='result'></div>", "https://example.test/", r#"
      const frame=document.createElement('iframe');document.body.appendChild(frame);
      frame.srcdoc='<head><title>child</title><style>.child {color:red}</style></head><body><p id="child">child</p>';
      frame.addEventListener('load',function(){document.getElementById('result').setAttribute('data-listener',String(this===frame)+':'+typeof frame.onload);});
      frame.onload=()=>{try {
        const child=frame.contentWindow,doc=child.document;
        const values=[doc.getElementById('child').textContent,doc.head.querySelector('title').textContent,doc.head.querySelector('style').sheet.cssRules.length,doc.getElementById('child').ownerDocument===doc,child.Event!==Event];
        document.getElementById('result').setAttribute('data-values',JSON.stringify(values));
      }catch(error){document.getElementById('result').setAttribute('data-error',String(error.stack||error));}};
    "#).await.unwrap();
    assert!(
        html.contains("data-values=\"[&quot;child&quot;,&quot;child&quot;,1,true,true]\""),
        "{html}"
    );
}
