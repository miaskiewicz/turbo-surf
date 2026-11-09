//! Classic execution errors report on Window without turning a fetched script into a load failure.
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const DOCUMENT: &str = r#"<!doctype html><body>
<script>
globalThis.errors=[];globalThis.resources=[];
addEventListener('error',event=>errors.push({
  trusted:event.isTrusted,identity:event.error===globalThis.originalError,
  ownRealm:event.error instanceof Error,source:event.filename,
  located:event.lineno>0&&event.colno>0,message:event.message
}));
globalThis.originalError=new Error('inline failure');throw originalError;
</script>
<script src='/throw.js' onload="resources.push(['throw','load',event.isTrusted])" onerror="resources.push(['throw','error',event.isTrusted])"></script>
<script src='/syntax.js' onload="resources.push(['syntax','load',event.isTrusted])" onerror="resources.push(['syntax','error',event.isTrusted])"></script>
<script>globalThis.continued=true;</script>
</body>"#;

async fn server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = vec![0; 8192];
                let count = socket.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (mime, body) = match path {
                    "/throw.js" => ("text/javascript", "globalThis.originalError=new Error('downloaded failure');throw originalError;"),
                    "/syntax.js" => ("text/javascript", "const = invalid;"),
                    _ => ("text/html", DOCUMENT),
                };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    (origin, task)
}

#[tokio::test(flavor = "current_thread")]
async fn main_and_child_classic_errors_preserve_identity_realm_and_resource_success() {
    let (origin, server) = server().await;
    let html = format!("{DOCUMENT}<iframe src='/frame'></iframe>");
    let mut page = PageSession::open(&html, &format!("{origin}/page"), "", "", 10000)
        .await
        .unwrap();
    let value: Value = serde_json::from_str(&page.eval_expression(r#"JSON.stringify([window,document.querySelector('iframe').contentWindow].map(win=>({
      continued:win.continued,resources:win.resources,errors:win.errors
    })))"#).await.unwrap()).unwrap();
    for (index, result) in value.as_array().unwrap().iter().enumerate() {
        assert_eq!(result["continued"], true);
        assert_eq!(
            result["resources"],
            json!([["throw", "load", true], ["syntax", "load", true]])
        );
        let errors = result["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 3);
        for (error_index, error) in errors.iter().enumerate() {
            assert_eq!(error["trusted"], true);
            assert_eq!(error["ownRealm"], true);
            assert_eq!(error["located"], true);
            let expected_message = [
                "Uncaught Error: inline failure",
                "Uncaught Error: downloaded failure",
                "Uncaught SyntaxError: Unexpected token '='",
            ][error_index];
            assert_eq!(error["message"], expected_message);
            assert_eq!(error["identity"], error_index < 2);
            let file = if error_index == 0 {
                if index == 0 {
                    "page"
                } else {
                    "frame"
                }
            } else if error_index == 1 {
                "throw.js"
            } else {
                "syntax.js"
            };
            assert_eq!(error["source"], format!("{origin}/{file}"));
        }
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn cross_origin_no_cors_errors_are_muted_while_cors_errors_keep_the_exception() {
    let (origin, server) = server().await;
    let (cross, cross_server) = self::server().await;
    let html = format!(
        r#"<body><script>
      globalThis.errors=[];globalThis.loads=0;
      addEventListener('error',event=>errors.push({{message:event.message,filename:event.filename,
        line:event.lineno,column:event.colno,error:event.error===null?'null':event.error===originalError?'original':'other',trusted:event.isTrusted}}));
      </script><script src='{cross}/throw.js' onload='loads++'></script>
      <script src='{cross}/throw.js' crossorigin=anonymous onload='loads++'></script>
      <iframe srcdoc="&lt;script&gt;globalThis.errors=[];globalThis.loads=0;addEventListener('error',event=&gt;errors.push([event.message,event.error===null,event.lineno,event.colno]));&lt;/script&gt;&lt;script src='{cross}/throw.js' onload='loads++'&gt;&lt;/script&gt;"></iframe></body>"#
    );
    let mut page = PageSession::open(&html, &format!("{origin}/page"), "", "", 10000)
        .await
        .unwrap();
    let value:Value=serde_json::from_str(&page.eval_expression(r#"JSON.stringify({errors,loads,child:document.querySelector('iframe').contentWindow.errors,childLoads:document.querySelector('iframe').contentWindow.loads})"#).await.unwrap()).unwrap();
    assert_eq!(value["loads"], 2);
    assert_eq!(
        value["errors"][0],
        json!({"message":"Script error.","filename":"","line":0,"column":0,"error":"null","trusted":true})
    );
    assert_eq!(
        value["errors"][1]["message"],
        "Uncaught Error: downloaded failure"
    );
    assert_eq!(value["errors"][1]["filename"], format!("{cross}/throw.js"));
    assert_eq!(value["errors"][1]["error"], "original");
    assert_eq!(value["errors"][1]["trusted"], true);
    assert!(value["errors"][1]["line"].as_u64().unwrap() > 0);
    assert!(value["errors"][1]["column"].as_u64().unwrap() > 0);
    assert_eq!(value["child"], json!([["Script error.", true, 0, 0]]));
    assert_eq!(value["childLoads"], 1);
    server.abort();
    cross_server.abort();
}
