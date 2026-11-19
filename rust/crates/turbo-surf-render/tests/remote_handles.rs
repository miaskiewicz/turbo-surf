use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn remote_values_are_private_live_and_disposable() {
    let mut page = PageSession::open("<b>ready</b>", "http://fixture.test/", "", "", 10_000)
        .await
        .unwrap();
    let id = page
        .eval_engine_expression(
            "__tsNs.remoteHandleRetain(globalThis, globalThis.shared={count:1})",
        )
        .await
        .unwrap();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 1);
    assert_eq!(
        page.eval_engine_expression("__tsNs.remoteHandleCount()")
            .await
            .unwrap(),
        "1"
    );
    page.eval_expression("shared.count=2").await.unwrap();
    assert_eq!(
        page.eval_engine_expression(&format!("__tsNs.remoteHandleGet({id}).count"))
            .await
            .unwrap(),
        "2"
    );
    assert_eq!(
        page.eval_expression("typeof globalThis.__tsNs")
            .await
            .unwrap(),
        "undefined"
    );
    page.eval_engine(&format!("__tsNs.remoteHandleDispose({id});"))
        .await
        .unwrap();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 0);
    assert!(page
        .eval_engine_expression(&format!("__tsNs.remoteHandleGet({id})"))
        .await
        .unwrap_err()
        .contains("disposed"));
}

#[tokio::test(flavor = "current_thread")]
async fn remote_handle_index_does_not_root_detached_frame_realms() {
    let mut page = PageSession::open(
        "<iframe srcdoc='<b>child</b>'></iframe>",
        "http://fixture.test/",
        "",
        "",
        10_000,
    )
    .await
    .unwrap();
    let id = page.eval_engine_expression("__tsNs.remoteHandleRetain(document.querySelector('iframe').contentWindow, document.querySelector('iframe').contentWindow.document.body)").await.unwrap();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 1);
    page.eval_expression("document.querySelector('iframe').remove()")
        .await
        .unwrap();
    assert!(page
        .eval_engine_expression(&format!("__tsNs.remoteHandleGet({id})"))
        .await
        .unwrap_err()
        .contains("context"));
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn native_form_navigation_never_reuses_remote_handle_ids() {
    use std::io::{Read, Write};
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/", server.local_addr().unwrap());
    let serving = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let mut request = [0; 8192];
        let length = stream.read(&mut request).unwrap();
        assert!(String::from_utf8_lossy(&request[..length]).starts_with("POST /after"));
        let body = "<html><head><title>after</title></head><body>new</body></html>";
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let mut page = PageSession::open(
        "<form action='/after' method=post></form>",
        &base,
        "",
        "",
        10_000,
    )
    .await
    .unwrap();
    let before = page
        .eval_engine_expression("__tsNs.remoteHandleRetain(globalThis,{value:'before'})")
        .await
        .unwrap();
    page.eval_expression("document.querySelector('form').requestSubmit()")
        .await
        .unwrap();
    let after = page
        .eval_engine_expression("__tsNs.remoteHandleRetain(globalThis,{value:'after'})")
        .await
        .unwrap();
    assert_ne!(before, after);
    assert!(page
        .eval_engine_expression(&format!("__tsNs.remoteHandleGet({before})"))
        .await
        .unwrap_err()
        .contains("context"));
    page.eval_engine(&format!("__tsNs.remoteHandleDispose({before});"))
        .await
        .unwrap();
    assert_eq!(
        page.eval_engine_expression(&format!("__tsNs.remoteHandleGet({after}).value"))
            .await
            .unwrap(),
        "after"
    );
    serving.join().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn trusted_frame_compilation_ignores_the_guest_function_constructor() {
    let mut page = PageSession::open(
        "<iframe srcdoc='<b>child</b>'></iframe>",
        "http://fixture.test/",
        "",
        "",
        10_000,
    )
    .await
    .unwrap();
    let root = page
        .eval_engine_expression("__tsNs.remoteHandleRetain(globalThis, {root:true})")
        .await
        .unwrap();
    assert!(page
        .eval_engine_expression(&format!(
            "__tsNs.remoteHandleGet({root},document.querySelector('iframe').contentWindow)"
        ))
        .await
        .unwrap_err()
        .contains("different execution context"));
    page.eval_expression("document.querySelector('iframe').contentWindow.Function=function(){throw new Error('guest constructor called')}").await.unwrap();
    assert_eq!(page.eval_engine_expression("__tsNs.remoteHandleCompile(document.querySelector('iframe').contentWindow, 'return document.body.textContent')()").await.unwrap(),"child");
    assert_eq!(page.eval_engine_expression("(()=>{const host=document.querySelector('iframe'),child=host.contentWindow;host.remove();try{__tsNs.remoteHandleCompile(child,'return 1');return false;}catch(error){return error.message.includes('context');}})()").await.unwrap(),"true");
}
