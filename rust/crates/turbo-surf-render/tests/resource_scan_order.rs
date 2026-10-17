//! Parser resource cutoffs retain live order when synchronous resource hooks move nodes.
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn resource_cutoff_observes_script_moves_and_excludes_svg_scripts() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let image_requested = Arc::new(tokio::sync::Semaphore::new(0));
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let observed = observed.clone();
            let image_requested = image_requested.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let count = socket.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    if bytes.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap().to_owned();
                observed.lock().unwrap().push(path.clone());
                let mut status = "200 OK";
                if path == "/gate.css"
                    && tokio::time::timeout(Duration::from_secs(5), image_requested.acquire())
                        .await
                        .is_err()
                {
                    status = "500 Image was not admitted before the parser script";
                }
                let (mime, body): (&str, &[u8]) = if path == "/moved.png" {
                    image_requested.add_permits(1);
                    (
                        "image/png",
                        include_bytes!("../../../../harness/detection/fixtures/pixel.png"),
                    )
                } else {
                    ("text/css", b"sentinel {color:blue}")
                };
                let headers = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                socket.write_all(headers.as_bytes()).await.unwrap();
                socket.write_all(body).await.unwrap();
            });
        }
    });
    let html = r#"<head>
      <link id=early rel=stylesheet href='/early.css'>
      <script>
      const upper=document.createElementNS('http://www.w3.org/1999/xhtml','SCRIPT');
      upper.setAttribute('type','application/json');upper.setAttribute('rel','stylesheet');upper.setAttribute('href','/unexpected-uppercase.css');document.head.appendChild(upper);
      const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');
      for(const name of ['script','svg:script']){const node=document.createElementNS('http://www.w3.org/2000/svg',name);node.setAttribute('type','application/json');node.setAttribute('rel','stylesheet');node.setAttribute('href','/unexpected.css');svg.appendChild(node);}
      document.head.appendChild(svg);
      globalThis.cutoff=[document.querySelector('#early').sheet.cssRules.length,document.querySelector('#later')?.sheet==null]</script>
      <link id=later rel=stylesheet href='/later.css'>
      <script>cutoff.push(document.querySelector('#later').sheet.cssRules.length)</script>
      </head><body>
      <script>globalThis.finished=true;globalThis.__browserFixture={cutoff,finished}</script>"#;
    let init = r#"const originalFrom=Array.from;Array.from=function(input,...args){
      const result=Reflect.apply(originalFrom,this,[input,...args]);
      if(input&&typeof input.item==='function'&&result.every(node=>['LINK','IMG','STYLE','SCRIPT'].includes(node.tagName))&&result.some(node=>node.tagName==='SCRIPT')&&result.some(node=>['LINK','IMG','STYLE'].includes(node.tagName)))result.length=0;
      return result;
    };
    const originalIterator=NodeList.prototype[Symbol.iterator];NodeList.prototype[Symbol.iterator]=function*(){
      const values=[...Reflect.apply(originalIterator,this,[])];
      if(values.every(node=>['LINK','IMG','STYLE','SCRIPT'].includes(node.tagName))&&values.some(node=>node.tagName==='SCRIPT')&&values.some(node=>['LINK','IMG','STYLE'].includes(node.tagName)))values.reverse();
      yield* values;
    };
"#;
    let mut page = PageSession::open_with_init_script(html, &base, "", "", 8000, init)
        .await
        .unwrap();
    assert!(
        !page.hydration_diagnostics().budget_exceeded,
        "{:?}",
        page.hydration_diagnostics()
    );
    let chrome: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/resource-scan-order-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        chrome["fixtureSha256"],
        format!("{:x}", Sha256::digest(html))
    );
    assert_eq!(chrome["errors"], serde_json::json!([]));
    assert_eq!(
        chrome["unexpectedStylesheetRequests"],
        serde_json::json!([])
    );
    assert_eq!(
        page.eval_json_expression("cutoff").await.unwrap(),
        chrome["result"]["cutoff"].to_string()
    );
    assert_eq!(page.eval_expression("finished").await.unwrap(), "true");
    page.close();
    assert!(!requests
        .lock()
        .unwrap()
        .iter()
        .any(|path| path.starts_with("/unexpected")));

    // The first stylesheet's synchronous disabled getter moves the parser script
    // past a later image. That image must be admitted before the stylesheet gate
    // can settle; the stale cutoff index would otherwise deadlock the parser.
    let html = r#"<head><link id=gate rel=stylesheet href='/gate.css'></head><body>
      <script id=cutoff>globalThis.movedCutoff=document.querySelector('#gate').sheet.cssRules.length</script>
      <img id=moved src='/moved.png'><script id=after>globalThis.afterCutoff=true</script>"#;
    let init = r#"let moved=false;Object.defineProperty(document.querySelector('#gate'),'disabled',{get(){
      if(!moved){moved=true;document.body.insertBefore(document.querySelector('#cutoff'),document.querySelector('#after'));}
      return false;
    }});"#;
    let mut page = PageSession::open_with_init_script(html, &base, "", "", 8000, init)
        .await
        .unwrap();
    assert!(
        !page.hydration_diagnostics().budget_exceeded,
        "{:?}",
        page.hydration_diagnostics()
    );
    assert_eq!(
        page.eval_json_expression("[movedCutoff,afterCutoff]")
            .await
            .unwrap(),
        "[1,true]"
    );
    assert_eq!(
        page.eval_json_expression("Array.from(document.body.children).map(node=>node.id)")
            .await
            .unwrap(),
        "[\"moved\",\"cutoff\",\"after\"]"
    );
    page.close();
    let html = "<body><svg><script type='application/json'></script></svg><script>globalThis.preservedParserScript=42</script>";
    let init = "const upper=document.createElementNS('http://www.w3.org/1999/xhtml','SCRIPT');upper.setAttribute('type','application/json');document.body.insertBefore(upper,document.body.firstChild);";
    let mut page = PageSession::open_with_init_script(html, &base, "", "", 8000, init)
        .await
        .unwrap();
    assert!(
        !page.hydration_diagnostics().budget_exceeded,
        "{:?}",
        page.hydration_diagnostics()
    );
    assert_eq!(
        page.eval_expression("preservedParserScript").await.unwrap(),
        "42"
    );
    page.close();
    server.abort();
}
