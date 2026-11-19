use serde_json::{json, Value};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_connectivity_updates_navigator_and_trusted_active_frame_events() {
    let hints = ClientHints::default();
    let mut page = PageSession::open_with_request_state(
        "<!doctype html><iframe srcdoc='<body>child</body>'></iframe>",
        "https://offline.test/",
        "",
        "",
        10_000,
        "",
        hints.clone(),
    )
    .await
    .unwrap();
    page.eval("for(const win of [window,document.querySelector('iframe').contentWindow]){win.netEvents=[];for(const type of ['offline','online'])win.addEventListener(type,event=>win.netEvents.push({type:event.type,trusted:event.isTrusted,online:win.navigator.onLine,bubbles:event.bubbles,cancelable:event.cancelable}));}").await.unwrap();
    hints.set_offline(true);
    page.eval_engine("__tsNs.syncNetworkState();")
        .await
        .unwrap();
    page.eval_engine("__tsNs.syncNetworkState();")
        .await
        .unwrap();
    let report:Value=serde_json::from_str(&page.eval_expression("JSON.stringify([window,document.querySelector('iframe').contentWindow].map(win=>({online:win.navigator.onLine,events:win.netEvents})))").await.unwrap()).unwrap();
    for row in report.as_array().unwrap() {
        assert_eq!(
            row,
            &json!({"online":false,"events":[{"type":"offline","trusted":true,"online":false,"bubbles":false,"cancelable":false}]})
        );
    }
    hints.set_offline(false);
    page.eval_engine("__tsNs.syncNetworkState();")
        .await
        .unwrap();
    let report:Value=serde_json::from_str(&page.eval_expression("JSON.stringify([window,document.querySelector('iframe').contentWindow].map(win=>win.netEvents.map(event=>[event.type,event.online,event.trusted])))").await.unwrap()).unwrap();
    assert_eq!(
        report,
        json!([
            [["offline", false, true], ["online", true, true]],
            [["offline", false, true], ["online", true, true]]
        ])
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn initial_offline_is_native_and_local_data_and_blob_stay_available() {
    let hints = ClientHints::default();
    hints.set_offline(true);
    let mut page = PageSession::open_with_request_state(
        "<!doctype html><body></body>",
        "https://offline.test/",
        "",
        "",
        10_000,
        "",
        hints,
    )
    .await
    .unwrap();
    assert_eq!(
        page.eval_expression("navigator.onLine").await.unwrap(),
        "false"
    );
    let report:Value=serde_json::from_str(&page.eval_expression("(async()=>{const data=await(await fetch('data:text/plain,data')).text();const url=URL.createObjectURL(new Blob(['blob']));const blob=await(await fetch(url)).text();URL.revokeObjectURL(url);return JSON.stringify({data,blob})})()").await.unwrap()).unwrap();
    assert_eq!(report, json!({"data":"data","blob":"blob"}));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn offline_blocks_private_resource_transports_but_not_an_already_loaded_module() {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut bytes = [0; 8192];
                let count = socket.read(&mut bytes).await.unwrap();
                let head = String::from_utf8_lossy(&bytes[..count]);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                log.lock().unwrap().push(path.to_owned());
                let body = "export const value=7;";
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: max-age=600\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let hints = ClientHints::default();
    let mut page = PageSession::open_with_request_state(
        "<!doctype html><body></body>",
        &format!("{origin}/page"),
        "",
        "",
        10_000,
        "",
        hints.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        page.eval_expression(&format!(
            "import('{origin}/warm.js').then(module=>module.value)"
        ))
        .await
        .unwrap(),
        "7"
    );
    hints.set_offline(true);
    page.eval_engine("__tsNs.syncNetworkState();")
        .await
        .unwrap();
    let script = format!(
        r#"(async()=>{{
      const element=(tag,attribute,url)=>new Promise(resolve=>{{const node=document.createElement(tag);node.onload=()=>resolve('load');node.onerror=()=>resolve('error');if(tag==='link')node.rel='stylesheet';node[attribute]=url;document.body.append(node)}});
      const classic=element('script','src','{origin}/cold-classic.js');
      const style=element('link','href','{origin}/cold.css');
      const image=new Promise(resolve=>{{const node=new Image();node.onload=()=>resolve('load');node.onerror=()=>resolve('error');node.src='{origin}/cold.png'}});
      const font=new FontFace('OfflineCold','url({origin}/cold.woff2)').load().then(()=> 'load',()=> 'error');
      const module=import('{origin}/cold-module.js').then(()=> 'load',()=> 'error');
      const request=fetch('{origin}/cold-fetch').then(()=> 'load',()=> 'error');
      const xhr=new Promise(resolve=>{{const request=new XMLHttpRequest();request.open('GET','{origin}/cold-xhr');request.onload=()=>resolve('load');request.onerror=()=>resolve('error');request.send()}});
      const frame=document.createElement('iframe');frame.src='{origin}/cold-frame';document.body.append(frame);
      const warm=(await import('{origin}/warm.js')).value;
      return JSON.stringify({{warm,results:await Promise.all([classic,style,image,font,module,request,xhr])}});
    }})()"#
    );
    let report: Value =
        serde_json::from_str(&page.eval_expression(&script).await.unwrap()).unwrap();
    assert_eq!(
        report,
        json!({"warm":7,"results":["error","error","error","error","error","error","error"]})
    );
    assert_eq!(*requests.lock().unwrap(), vec!["/warm.js"]);
    page.close();
    server.abort();
}
