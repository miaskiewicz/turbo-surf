//! Standalone async evaluation must retain native resource execution semantics.
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn script_server() -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut chunk = [0; 1024];
                    let length = socket.read(&mut chunk).await.unwrap();
                    assert_ne!(length, 0, "request ended before complete headers");
                    request.extend_from_slice(&chunk[..length]);
                    assert!(request.len() <= 8192, "oversized request headers");
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap().to_owned();
                seen.lock().unwrap().push(path.clone());
                let (status, body) = match path.as_str() {
                    "/good.js" => ("200 OK", "globalThis.executions.push([document.currentScript.id,document.currentScript.src]);"),
                    "/throw.js" => ("200 OK", "globalThis.downloadedError=new Error('resource execution');throw downloadedError;"),
                    _ => ("404 Not Found", "globalThis.mustNotRun=true;"),
                };
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    (origin, requests, task)
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_dynamic_classic_scripts_keep_element_identity_and_restore_current_script() {
    let (origin, requests, server) = script_server().await;
    let raw = turbo_surf_render::eval_async("<body>", &format!("{origin}/document"), r#"
      globalThis.executions=[];
      (async()=>{
        const script=document.createElement('script');script.id='download';script.src='/good.js';
        const loaded=new Promise((resolve,reject)=>{script.onload=()=>resolve();script.onerror=()=>reject(new Error('load failed'));});
        document.body.append(script);await loaded;
        globalThis.__RESULT=JSON.stringify({executions,current:document.currentScript===null,connected:script.isConnected});
      })();
    "#).await.unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value,
        json!({"executions":[["download",format!("{origin}/good.js")]],"current":true,"connected":true})
    );
    assert_eq!(*requests.lock().unwrap(), ["/good.js"]);
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_classic_execution_error_is_distinct_from_failed_resource_admission() {
    let (origin, requests, server) = script_server().await;
    let raw = turbo_surf_render::eval_async("<body>", &format!("{origin}/document"), r#"
      globalThis.errors=[];globalThis.events=[];
      addEventListener('error',e=>errors.push({identity:e.error===globalThis.downloadedError,message:e.message,filename:e.filename}));
      (async()=>{
        for(const path of ['/throw.js','/missing.js']){
          const script=document.createElement('script');script.src=path;
          await new Promise(resolve=>{script.onload=()=>{events.push([path,'load']);resolve();};script.onerror=()=>{events.push([path,'error']);resolve();};document.body.append(script);});
        }
        globalThis.__RESULT=JSON.stringify({errors,events,current:document.currentScript===null,forbidden:globalThis.mustNotRun===true});
      })();
    "#).await.unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value["events"],
        json!([["/throw.js", "load"], ["/missing.js", "error"]])
    );
    assert_eq!(
        value["errors"],
        json!([{"identity":true,"message":"Uncaught Error: resource execution","filename":format!("{origin}/throw.js")} ])
    );
    assert_eq!(value["current"], true);
    assert_eq!(value["forbidden"], false);
    assert_eq!(*requests.lock().unwrap(), ["/throw.js", "/missing.js"]);
    server.abort();
}
