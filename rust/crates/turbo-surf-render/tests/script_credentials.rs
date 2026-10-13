use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn script_module_credentials_and_redirect_checks_are_real_per_request() {
    let source = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cross = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source_url = format!("http://{}", source.local_addr().unwrap());
    let cross_url = format!("http://{}", cross.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut servers = Vec::new();
    for (listener, is_source) in [(source, true), (cross, false)] {
        let source_url = source_url.clone();
        let requests = requests.clone();
        servers.push(tokio::spawn(async move {
        while let Ok((mut socket,_))=listener.accept().await {
          let source_url=source_url.clone();let requests=requests.clone();
          tokio::spawn(async move {
            let mut bytes=Vec::new();loop {let mut chunk=[0;1024];let n=socket.read(&mut chunk).await.unwrap();if n==0{return;}bytes.extend_from_slice(&chunk[..n]);if bytes.windows(4).any(|part|part==b"\r\n\r\n"){break;}}
            let request=String::from_utf8_lossy(&bytes).to_string();let path=request.split_whitespace().nth(1).unwrap_or("/");
            requests.lock().unwrap().push((is_source,path.to_string(),request.clone()));
            let (status,headers,body)=match (is_source,path) {
              (false,"/entry.mjs")=>(200,format!("Access-Control-Allow-Origin: {source_url}\r\nAccess-Control-Allow-Credentials: true\r\n"),include_str!("../../../../harness/detection/fixtures/script-credentials/entry.mjs").to_string()),
              (false,"/dep.mjs")=>(200,format!("Access-Control-Allow-Origin: {source_url}\r\nAccess-Control-Allow-Credentials: true\r\n"),include_str!("../../../../harness/detection/fixtures/script-credentials/dep.mjs").to_string()),
              (false,"/anonymous.mjs")=>(200,"Access-Control-Allow-Origin: *\r\n".into(),"globalThis.anonymousModule=true;".into()),
              (false,"/classic.js")=>(200,format!("Access-Control-Allow-Origin: {source_url}\r\nAccess-Control-Allow-Credentials: true\r\n"),"globalThis.credentialClassic=true;".into()),
              (false,"/nosniff.js")=>(200,"X-Content-Type-Options: nosniff\r\n".into(),"globalThis.nosniffExecuted=true;".into()),
              (false,"/legacy.js")=>(200,String::new(),"globalThis.legacyMime=true;".into()),
              (false,"/json.mjs")=>(200,"Access-Control-Allow-Origin: *\r\n".into(),"globalThis.jsonExecuted=true;".into()),
              (false,"/denied")=>(302,format!("Location: {source_url}/must-not-request.mjs\r\n"),String::new()),
              (false,"/redirect")=>(302,format!("Access-Control-Allow-Origin: {source_url}\r\nAccess-Control-Allow-Credentials: true\r\nLocation: {source_url}/final/entry.mjs\r\n"),String::new()),
              (true,"/final/entry.mjs")=>(200,"Access-Control-Allow-Origin: null\r\nAccess-Control-Allow-Credentials: true\r\n".into(),"import {value} from './dep.mjs';globalThis.redirectModule={value,url:new URL(import.meta.url).pathname};".into()),
              (true,"/final/dep.mjs")=>(200,String::new(),"export const value='redirect dependency';".into()),
              _=>(404,String::new(),String::new()),
            };
            let mime=if matches!(path,"/nosniff.js"|"/legacy.js"){"text/plain"}else if path=="/json.mjs"{"application/json"}else{"text/javascript"};
            socket.write_all(format!("HTTP/1.1 {status} OK\r\nContent-Type: {mime}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
          });
        }
      }));
    }
    let mut jar = turbo_surf_core::cookies::CookieJar::new();
    jar.set_from_response(
        &source_url,
        &["secret=session; HttpOnly; Path=/".into()],
        0.0,
    );
    assert!(jar
        .cookie_header(&source_url, 0.0)
        .contains("secret=session"));
    let html = include_str!("../../../../harness/detection/fixtures/script-credentials/page.html")
        .replace("{{CROSS}}", &cross_url);
    let mut page = PageSession::open(
        &html,
        &format!("{source_url}/page"),
        &jar.storage_state(),
        "",
        10000,
    )
    .await
    .unwrap();
    let result = page
        .eval("globalThis.__RESULT=JSON.stringify(__browserFixture)")
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value["module"]["value"], "credentialed dependency",
        "{value}"
    );
    assert_eq!(value["module"]["current"], true);
    assert_eq!(value["anonymous"], true, "{value}");
    for key in ["legacy", "nosniff", "json"] {
        assert_eq!(value[key], true, "{key}: {value}");
    }
    assert_eq!(value["classic"], true, "{value}");
    assert_eq!(value["cookie"], "");
    assert_eq!(value["redirect"]["url"], "/final/entry.mjs", "{value}");
    let events = value["events"].as_array().expect("script handler events");
    assert_eq!(events.len(), 8, "{value}");
    for (id, event_type) in [
        ("module", "load"),
        ("anonymous", "load"),
        ("classic", "load"),
        ("redirect", "load"),
        ("legacy", "load"),
        ("denied", "error"),
        ("nosniff", "error"),
        ("json", "error"),
    ] {
        assert!(
            events
                .iter()
                .any(|event| event[0] == id && event[1] == event_type && event[2] == true),
            "{value}"
        );
    }
    let requests = requests.lock().unwrap();
    for path in ["/entry.mjs", "/dep.mjs", "/classic.js"] {
        let request = &requests
            .iter()
            .find(|(source, url, _)| !source && url == path)
            .unwrap()
            .2;
        assert!(
            request
                .to_ascii_lowercase()
                .contains("cookie: secret=session"),
            "{request}"
        );
        assert!(
            request.contains(&format!("origin: {source_url}")),
            "{request}"
        );
    }
    let anonymous = &requests
        .iter()
        .find(|(_, path, _)| path == "/anonymous.mjs")
        .unwrap()
        .2;
    assert!(
        !anonymous.to_ascii_lowercase().contains("cookie:"),
        "{anonymous}"
    );
    assert!(!requests
        .iter()
        .any(|(_, path, _)| path == "/must-not-request.mjs"));
    let redirected = &requests
        .iter()
        .find(|(_, path, _)| path == "/final/entry.mjs")
        .unwrap()
        .2;
    assert!(
        redirected.to_ascii_lowercase().contains("origin: null"),
        "{redirected}"
    );
    for server in servers {
        server.abort();
    }
}
