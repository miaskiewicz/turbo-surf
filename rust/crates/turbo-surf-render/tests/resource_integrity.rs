use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const SCRIPT: &str = "globalThis.integrityScript=(globalThis.integrityScript||0)+1;";
const MODULE: &str =
    "import './dep.mjs';globalThis.integrityModule=(globalThis.integrityModule||0)+1;";
const STYLE: &str = "body { color: rgb(12, 34, 56); }";

async fn server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let count = socket.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..count]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (mime, body): (&str, &[u8]) = match path {
                    "/data" => ("application/octet-stream", b"raw\xffbytes"),
                    "/entry.mjs" | "/blocked.mjs" => ("text/javascript", MODULE.as_bytes()),
                    "/dep.mjs" => ("text/javascript", b"globalThis.integrityDependency=true;"),
                    "/style.css" => ("text/css", STYLE.as_bytes()),
                    "/blocked.css" => ("text/css", b"body { background-color: red; }"),
                    "/frame" => (
                        "text/html",
                        b"<body><script>globalThis.integrityErrors=0;</script><script src='/script.js' integrity='sha256-YQ==' onerror='integrityErrors++'></script></body>",
                    ),
                    _ => ("text/javascript", SCRIPT.as_bytes()),
                };
                let policy = match path {
                    "/corp-origin.js" => "Cross-Origin-Resource-Policy: same-origin\r\n",
                    "/corp-site.js" => "Cross-Origin-Resource-Policy: same-site\r\n",
                    "/corp-cross.js" => "Cross-Origin-Resource-Policy: cross-origin\r\n",
                    "/corp-invalid.js" => "Cross-Origin-Resource-Policy: Same-Origin\r\n",
                    "/corp-duplicate.js" => "Cross-Origin-Resource-Policy: same-origin\r\nCross-Origin-Resource-Policy: same-site\r\n",
                    _ => "",
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nAccess-Control-Allow-Origin: *\r\n{policy}Content-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
                socket.write_all(body).await.unwrap();
            });
        }
    });
    (origin, handle)
}

fn integrity(bytes: &[u8]) -> String {
    format!("sha256-{}", STANDARD.encode(Sha256::digest(bytes)))
}

#[tokio::test(flavor = "current_thread")]
async fn fetch_checks_raw_bytes_strongest_hashes_and_opaque_eligibility() {
    let (origin, server) = server().await;
    let (cross, cross_server) = self::server().await;
    let mut page = PageSession::open("<body></body>", &format!("{origin}/page"), "", "", 5000)
        .await
        .unwrap();
    let script = format!(
        r#"(async()=>{{
      const outcomes=[];
      for(const [url,options] of [
        ['/data',{{integrity:{hash:?}}}],
        ['/data',{{integrity:'sha256-YQ=='}}],
        ['/data',{{integrity:{stronger:?}}}],
        ['/data',{{integrity:'unsupported-YQ=='}}],
        [{cross:?}+'/data',{{mode:'cors',integrity:{hash:?}}}],
        [{cross:?}+'/data',{{mode:'no-cors',integrity:{hash:?}}}],
        [{cross:?}+'/data',{{mode:'no-cors',integrity:'unsupported-YQ=='}}]
      ]){{try{{const response=await fetch(url,options);outcomes.push(Array.from(new Uint8Array(await response.arrayBuffer())))}}catch(error){{outcomes.push(error.name)}}}}
      return JSON.stringify(outcomes);
    }})()"#,
        hash = integrity(b"raw\xffbytes"),
        stronger = format!("{} sha512-YQ==", integrity(b"raw\xffbytes"))
    );
    let value: Value = serde_json::from_str(&page.eval_expression(&script).await.unwrap()).unwrap();
    let bytes = json!([114, 97, 119, 255, 98, 121, 116, 101, 115]);
    assert_eq!(
        value,
        json!([
            bytes,
            "TypeError",
            "TypeError",
            bytes,
            bytes,
            "TypeError",
            "TypeError"
        ])
    );
    server.abort();
    cross_server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_controls_script_module_stylesheet_and_child_execution() {
    let (origin, server) = server().await;
    let html = format!(
        r#"<!doctype html><head>
      <link rel=stylesheet href=/style.css integrity='{style}'>
      <script>globalThis.loads=[];document.addEventListener('error',event=>loads.push(['error',event.target.tagName]),true);document.addEventListener('load',event=>{{if(event.target.tagName)loads.push(['load',event.target.tagName])}},true);</script>
      <link id=blocked-style rel=stylesheet href=/blocked.css integrity='sha256-YQ=='>
      <script src=/script.js integrity='{script}'></script>
      <script src=/blocked.js integrity='sha256-YQ=='></script>
      <script type=module src=/entry.mjs integrity='{module}'></script>
      <script type=module src=/blocked.mjs integrity='sha256-YQ=='></script>
      </head><body><iframe src=/frame></iframe></body>"#,
        style = integrity(STYLE.as_bytes()),
        script = integrity(SCRIPT.as_bytes()),
        module = integrity(MODULE.as_bytes())
    );
    let mut page = PageSession::open(&html, &format!("{origin}/page"), "", "", 10000)
        .await
        .unwrap();
    let value: Value = serde_json::from_str(
        &page
            .eval_expression(
                r#"JSON.stringify({
      script:integrityScript,module:integrityModule,dependency:integrityDependency,
      child:typeof document.querySelector('iframe').contentWindow.integrityScript,
      style:document.styleSheets[0].cssRules[0].style.color,
      blockedSheet:document.getElementById('blocked-style').sheet,
      styleErrors:loads.filter(row=>row[0]==='error'&&row[1]==='LINK').length,
      childErrors:document.querySelector('iframe').contentWindow.integrityErrors,
      scriptErrors:loads.filter(row=>row[0]==='error'&&row[1]==='SCRIPT').length
    })"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        value,
        json!({"script":1,"module":1,"dependency":true,"child":"undefined","style":"rgb(12, 34, 56)","blockedSheet":null,"styleErrors":1,"childErrors":1,"scriptErrors":2})
    );
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn corp_controls_native_opaque_fetch_and_classic_script_admission() {
    let (origin, server) = server().await;
    let (cross, cross_server) = self::server().await;
    let html = format!(
        r#"<body>
      <script src='{cross}/corp-origin.js'></script>
      <script src='{cross}/corp-site.js'></script>
      <script src='{cross}/corp-cross.js'></script>
      <script src='{cross}/corp-origin.js' crossorigin></script>
      </body>"#
    );
    let mut page = PageSession::open(&html, &format!("{origin}/page"), "", "", 5000)
        .await
        .unwrap();
    let value: Value = serde_json::from_str(&page.eval_expression(&format!(r#"(async()=>{{
      const result=[];
      for(const [path,mode] of [
        ['/corp-origin.js','no-cors'],['/corp-site.js','no-cors'],
        ['/corp-cross.js','no-cors'],['/corp-invalid.js','no-cors'],
        ['/corp-duplicate.js','no-cors'],['/corp-origin.js','cors']
      ]){{try{{result.push((await fetch({cross:?}+path,{{mode}})).type)}}catch(error){{result.push(error.name)}}}}
      return JSON.stringify([integrityScript,result]);
    }})()"#)).await.unwrap()).unwrap();
    assert_eq!(
        value,
        json!([
            3,
            ["TypeError", "opaque", "opaque", "opaque", "opaque", "cors"]
        ])
    );
    page.close();
    let local_host = origin.replace("127.0.0.1", "localhost");
    let mut page = PageSession::open("<body></body>", &format!("{local_host}/page"), "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval_expression(&format!(r#"fetch({cross:?}+'/corp-site.js',{{mode:'no-cors'}}).then(()=> 'unexpected',error=>error.name)"#)).await.unwrap(), "TypeError");
    server.abort();
    cross_server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn active_mixed_content_is_blocked_before_a_request_and_loopback_remains_usable() {
    let (origin, server) = server().await;
    let mut page = PageSession::open("<body></body>", "https://secure.test/page", "", "", 5000)
        .await
        .unwrap();
    let value: Value = serde_json::from_str(&page.eval_expression(&format!(r#"(async()=>{{
      const blocked=await fetch('http://outside.invalid/file').then(()=> 'unexpected',error=>error.name);
      const loopback=await fetch({origin:?}+'/data').then(response=>response.status,error=>error.name);
      let reads=0;const request=new Request('/file',{{get integrity(){{reads++;return null}}}});
      return JSON.stringify([blocked,loopback,request.integrity,reads]);
    }})()"#)).await.unwrap()).unwrap();
    assert_eq!(value, json!(["TypeError", 200, "null", 1]));
    server.abort();
}
