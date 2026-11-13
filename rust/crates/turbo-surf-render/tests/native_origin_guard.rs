use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

async fn start_server() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            hits.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut bytes = vec![0; 8192];
                let n = socket.read(&mut bytes).await.unwrap();
                let head = String::from_utf8_lossy(&bytes[..n]);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let (mime, body) = match path {
                    "/classic.js" => (
                        "text/javascript",
                        "globalThis.classicLoaded=(globalThis.classicLoaded||0)+1",
                    ),
                    "/module.js" => (
                        "text/javascript",
                        "globalThis.moduleLoaded=true;export const loaded=true;",
                    ),
                    "/style.css" => ("text/css", "body{color:rgb(1,2,3)}"),
                    _ => ("text/html", "<!doctype html><p>real local resource</p>"),
                };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    (origin, count, task)
}

#[tokio::test(flavor = "current_thread")]
async fn native_resources_cannot_escape_host_origins_through_private_fetches() {
    const CHILD: &str = "SURF_RENDER_ORIGIN_POLICY_TEST";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_resources_cannot_escape_host_origins_through_private_fetches",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let diagnostics = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !diagnostics.contains("SURF_SECRET_SENTINEL_505"),
            "blocked resource diagnostics must redact URL queries"
        );
        assert!(output.status.success(), "{diagnostics}");
        return;
    }
    let (allowed, hits, server) = start_server().await;
    let (denied, denied_hits, denied_server) = start_server().await;
    std::env::set_var(
        "TURBO_SURF_ALLOWED_ORIGINS",
        serde_json::to_string(&[&allowed]).unwrap(),
    );
    let html = format!(
        r#"<!doctype html><html><head>
<script src="{allowed}/classic.js"></script>
<script src="{denied}/classic.js?key=SURF_SECRET_SENTINEL_505"></script>
<script type="module" src="{denied}/module.js?key=SURF_SECRET_SENTINEL_505"></script>
<link rel="stylesheet" href="{denied}/style.css">
</head><body><iframe src="{denied}/frame"></iframe><img src="{denied}/image.png"></body></html>"#
    );
    let mut page = PageSession::open(&html, &format!("{allowed}/page"), "", "", 10_000)
        .await
        .unwrap();
    let probe = format!(
        r#"(async()=>{{
      const dynamic=new Promise(resolve=>{{const script=document.createElement('script');script.src='{denied}/dynamic.js?key=SURF_SECRET_SENTINEL_505';script.onload=()=>resolve('load');script.onerror=()=>resolve('error');document.head.append(script)}});
      const style=new Promise(resolve=>{{const link=document.createElement('link');link.rel='stylesheet';link.href='{denied}/dynamic.css';link.onload=()=>resolve('load');link.onerror=()=>resolve('error');document.head.append(link)}});
      const font=new FontFace('BlockedHostFont','url({denied}/font.woff2)').load().then(()=> 'load',()=> 'error');
      const module=import('{denied}/module.js?key=SURF_SECRET_SENTINEL_505').then(()=> 'load',()=> 'error');
      const request=fetch('{denied}/fetch').then(()=> 'load',()=> 'error');
      const xhr=new Promise(resolve=>{{const x=new XMLHttpRequest();x.open('GET','{denied}/xhr');x.onload=()=>resolve('load');x.onerror=()=>resolve('error');x.send()}});
      const data=await (await fetch('data:text/plain,inline-data')).text();
      const blobURL=URL.createObjectURL(new Blob(['inline-blob']));
      const blob=await (await fetch(blobURL)).text();URL.revokeObjectURL(blobURL);
      const image=await new Promise(resolve=>{{const img=new Image();img.onload=()=>resolve(img.naturalWidth);img.onerror=()=>resolve('error');img.src='data:image/svg+xml,'+encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"></svg>')}});
      return JSON.stringify({{allowed:classicLoaded,data,blob,image,results:await Promise.all([dynamic,style,font,module,request,xhr])}});
    }})()"#
    );
    let report: serde_json::Value =
        serde_json::from_str(&page.eval_expression(&probe).await.unwrap()).unwrap();
    assert_eq!(report["allowed"], 1, "{report}");
    assert_eq!(report["data"], "inline-data");
    assert_eq!(report["blob"], "inline-blob");
    assert_eq!(report["image"], 1);
    assert_eq!(
        report["results"],
        serde_json::json!(["error", "error", "error", "error", "error", "error"]),
        "{report}"
    );
    assert!(hits.load(Ordering::SeqCst) >= 1);
    assert_eq!(
        denied_hits.load(Ordering::SeqCst),
        0,
        "private resource path contacted denied peer"
    );
    page.close();
    server.abort();
    denied_server.abort();
}
