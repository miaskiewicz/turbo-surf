#![cfg(turbo_surf_native_proxy)]
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(flavor = "current_thread")]
async fn frame_content_viewports_resize_through_native_boxes_and_retired_documents_have_no_geometry(
) {
    let source = include_str!("../../../../harness/detection/frame-viewport-lifecycle-probe.js");
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-viewport-lifecycle-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    request.push(socket.read_u8().await.unwrap());
                    assert!(request.len() < 16384);
                }
                let body = "<!doctype html><html><head></head><body></body></html>";
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        &url,
        "",
        "",
        30000,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &page
            .eval_json_expression(source.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, control["result"]);
    page.close();
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn frame_script_startup_does_not_eagerly_require_parent_layout() {
    let source = include_str!("../../../../harness/detection/frame-layout-startup-probe.js");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-layout-startup-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "https://startup.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &page
            .eval_json_expression(source.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual["started"], expected["result"]["started"]);
    assert!(
        actual["initial"]["error"]
            .as_str()
            .unwrap()
            .contains("font-face resource is unsupported"),
        "{actual}"
    );
    assert_eq!(
        actual["widthAfterClear"],
        expected["result"]["widthAfterClear"]
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn failed_replacement_initialization_releases_roots_and_same_host_can_retry() {
    let mut page =
        turbo_surf_render::PageSession::open("<body>", "https://rollback.test/", "", "", 10000)
            .await
            .unwrap();
    page.fail_next_frame_initialization_for_testing();
    assert_eq!(page.eval_expression("globalThis.retryFrame=document.createElement('iframe');document.body.append(retryFrame);'attempted'").await.unwrap(),"attempted");
    assert!(page.take_frame_initialization_failure_for_testing());
    assert!(!page.take_frame_initialization_failure_for_testing());
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing(),
        serde_json::json!({"hostContexts":0,"retiredHostContexts":0,"queuedWindowMessages":0,"queuedWindowBytes":0})
    );
    assert_eq!(
        page.eval_expression("String(retryFrame.contentWindow!==null)")
            .await
            .unwrap(),
        "true"
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        1
    );
    page.eval_expression("retryFrame.remove();'removed'")
        .await
        .unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn failed_later_frame_navigation_can_recover_on_the_same_host() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=retryFrame></iframe>",
        "https://rollback.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        1
    );
    page.eval_expression("globalThis.retryFrame=document.getElementById('retryFrame');'ready'")
        .await
        .unwrap();
    page.fail_next_frame_initialization_for_testing();
    page.eval_expression("retryFrame.srcdoc='<body>failed';'attempted'")
        .await
        .unwrap();
    assert!(page.take_frame_initialization_failure_for_testing());
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    page.eval_expression("retryFrame.srcdoc='<body>recovered';'retried'")
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression("retryFrame.contentDocument.body.textContent")
            .await
            .unwrap(),
        "recovered"
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        1
    );
    page.eval_expression("retryFrame.remove();'removed'")
        .await
        .unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    page.close();
}
