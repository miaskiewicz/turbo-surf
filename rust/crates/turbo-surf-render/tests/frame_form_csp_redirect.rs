use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn a_child_form_redirect_block_keeps_its_document_and_reports_the_initial_action() {
    let blocked = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    blocked.set_nonblocking(true).unwrap();
    let blocked_url = format!("http://{}/destination", blocked.local_addr().unwrap());
    let source = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", source.local_addr().unwrap());
    let server = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(10), async move {
            let (mut stream, _) = source.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0, "form request ended before its headers");
                request.extend_from_slice(&chunk[..count]);
            }
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        })
        .await
        .expect("form request must reach its allowed initial origin")
    });
    let mut page = PageSession::open_with_response_headers(
        "<iframe name='destination' srcdoc='<p id=original>original</p>'></iframe><form action='/redirect' target='destination'></form>",
        &format!("{base}/page"),
        "",
        "",
        10_000,
        "",
        Default::default(),
        &[("content-security-policy".into(), "form-action 'self'".into())],
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(
            r#"(async()=>{
                const frame=document.querySelector('iframe');
                let loads=0;
                frame.addEventListener('load',()=>loads++);
                const violation=new Promise(resolve=>document.addEventListener('securitypolicyviolation',event=>resolve({directive:event.effectiveDirective,disposition:event.disposition,blocked:event.blockedURI}),{once:true}));
                document.querySelector('form').requestSubmit();
                const report=await violation;
                await new Promise(resolve=>setTimeout(resolve,0));
                return {report,loads,text:frame.contentDocument.getElementById('original').textContent};
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&raw).unwrap(),
        serde_json::json!({
            "report":{"directive":"form-action","disposition":"enforce","blocked":format!("{base}/redirect?")},
            "loads":0,
            "text":"original"
        })
    );
    let request = server.await.unwrap();
    let target = request.split_whitespace().nth(1).unwrap();
    assert_eq!(target.trim_end_matches('?'), "/redirect");
    assert!(
        matches!(blocked.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}
