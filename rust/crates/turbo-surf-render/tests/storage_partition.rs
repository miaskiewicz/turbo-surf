use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::{BrowserStorage, PageSession};

#[tokio::test(flavor = "current_thread")]
async fn storage_units_partitioning_and_access_match_source_pinned_chrome() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut bytes = [0; 8192];
                let count = socket.read(&mut bytes).await.unwrap();
                let middle = String::from_utf8_lossy(&bytes[..count]).contains(" /middle ");
                let body = if middle {
                    format!("<iframe src=\"http://127.0.0.1:{port}/widget\"></iframe><script>addEventListener('message',event=>parent.postMessage(event.data,'*'))</script>")
                } else {
                    format!(
                        "<!doctype html><script>{}</script>",
                        include_str!("../../../../harness/detection/storage-widget-probe.js")
                    )
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
            });
        }
    });
    let storage = BrowserStorage::default();
    let mut actual = Vec::new();
    let probe = include_str!("../../../../harness/detection/storage-probe.js")
        .trim()
        .trim_end_matches(';');
    for host in ["localhost", "127.0.0.1", "localhost"] {
        let mut page = PageSession::open_with_browsing_state(
            "<!doctype html><body>Storage control",
            &format!("http://{host}:{port}/"),
            "",
            "",
            15000,
            "",
            ClientHints::default(),
            &[],
            storage.clone(),
        )
        .await
        .unwrap();
        let result=page.eval(&format!("(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})().catch(e=>{{globalThis.__RESULT=JSON.stringify({{error:String(e),stack:e.stack}});}})" )).await.unwrap();
        actual.push(serde_json::from_str::<serde_json::Value>(&result).unwrap());
    }
    server.abort();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/storage-chrome155.json"
    ))
    .unwrap();
    assert_eq!(serde_json::json!(actual), control["result"]);
}

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn storage_handles_do_not_root_retired_frame_realms() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://storage-lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const handle=await child.document.requestStorageAccess({localStorage:true});
      handle.localStorage.setItem('retained','value');
      globalThis.keptStorage=handle.localStorage;
      globalThis.storageRealmRefs=[new WeakRef(child),new WeakRef(handle)];frame.remove();
      globalThis.__RESULT=keptStorage.getItem('retained');
    })()"#).await.unwrap();
    assert_eq!(
        page.eval_expression("keptStorage.getItem('retained')")
            .await
            .unwrap(),
        "value"
    );
    page.eval_expression("globalThis.keptStorage=null")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression(
            "JSON.stringify(storageRealmRefs.map(reference=>reference.deref()===undefined))"
        )
        .await
        .unwrap(),
        "[true,true]"
    );
}
