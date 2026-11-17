//! Public DOM callers cannot manufacture a resource-consumer document owner.
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn resource_consumers_reject_fake_receivers_and_retired_documents_and_use_adopted_owner() {
    let paths = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = paths.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut request = vec![0; 8192];
                let count = socket.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                seen.lock().unwrap().push(path.clone());
                let png: &[u8] = &[
                    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0,
                    0, 2, 8, 6, 0, 0, 0, 157, 116, 102, 26, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156,
                    99, 248, 207, 192, 240, 31, 134, 25, 144, 57, 0, 155, 126, 11, 245, 114, 176,
                    185, 60, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
                ];
                let (mime, body) = if path.ends_with(".png") {
                    ("image/png", png)
                } else {
                    ("text/html", b"<!doctype html><body>".as_slice())
                };
                let header=format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                socket.write_all(header.as_bytes()).await.unwrap();
                socket.write_all(body).await.unwrap();
            });
        }
    });
    let mut page = PageSession::open(
        "<body><iframe src='/child/frame'></iframe>",
        &format!("{origin}/root/page"),
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let result=page.eval_json_expression(r#"(async()=>{
      const frame=document.querySelector('iframe'),child=frame.contentWindow;
      const failures=[];
      for(const operation of [()=>Document.prototype.createElement.call({},'img'),()=>Element.prototype.setAttribute.call({},'src','/forged.png')]){
        try{operation();failures.push('accepted');}catch(error){failures.push(error.name);}
      }
      const adopted=child.document.createElement('img');document.body.append(adopted);
      const loaded=new Promise((resolve,reject)=>{adopted.onload=()=>resolve(true);adopted.onerror=()=>reject(Error('adopted image failed'));});
      adopted.setAttribute('src','asset.png');await loaded;
      const owner=adopted.ownerDocument===document,width=adopted.naturalWidth;
      const retired=child.document.createElement('img');child.document.body.append(retired);frame.remove();
      retired.setAttribute('src','/retired.png');
      const progress=document.createElement('img');document.body.append(progress);
      const progressed=new Promise((resolve,reject)=>{progress.onload=resolve;progress.onerror=()=>reject(Error('progress image failed'));});
      progress.src='progress.png';await progressed;
      await new Promise(resolve=>setTimeout(resolve,0));
      return {failures,owner,width,root:document.body.contains(adopted)};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"failures":["TypeError","TypeError"],"owner":true,"width":3,"root":true})
    );
    assert_eq!(
        *paths.lock().unwrap(),
        vec!["/child/frame", "/root/asset.png", "/root/progress.png"]
    );
    server.abort();
}
