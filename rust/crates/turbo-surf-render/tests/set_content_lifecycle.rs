use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn domcontentloaded_content_replacement_does_not_wait_for_an_async_script() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let (release, released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(10), async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
            }
            assert!(String::from_utf8(request)
                .unwrap()
                .starts_with("GET /late.js HTTP/1.1\r\n"));
            released.await.unwrap();
            let script = "globalThis.lateScriptRan=true";
            stream
                .write_all(
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{script}", script.len())
                        .as_bytes(),
                )
                .await
                .unwrap();
        })
        .await
        .expect("async script transport must settle")
    });
    let mut page = PageSession::open("<body>old</body>", &base, "", "", 10_000)
        .await
        .unwrap();
    page.set_content(
        "<script>globalThis.contentEvents=[];document.addEventListener('DOMContentLoaded',()=>contentEvents.push('interactive'),{once:true});window.addEventListener('load',()=>contentEvents.push('load'),{once:true});</script><script async src='/late.js'></script><p>new</p>",
        "domcontentloaded",
        Some(5_000),
    )
    .await
    .unwrap();
    let before: serde_json::Value = serde_json::from_str(
        &page.collect_engine("globalThis.__RESULT=JSON.stringify({events:contentEvents.slice(),state:document.readyState,late:globalThis.lateScriptRan===true})")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        before,
        serde_json::json!({"events":["interactive"],"state":"interactive","late":false})
    );
    release.send(()).unwrap();
    let after = page
        .eval_json_expression("new Promise(resolve=>window.addEventListener('load',()=>resolve({events:contentEvents.slice(),state:document.readyState,late:lateScriptRan}),{once:true}))")
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&after).unwrap(),
        serde_json::json!({"events":["interactive","load"],"state":"complete","late":true})
    );
    server.await.unwrap();
}
