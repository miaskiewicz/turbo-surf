//! Auxiliary navigation slices preserve complete callbacks and the caller deadline.
use serde_json::Value;
use turbo_surf_render::{PageSession, PageSessionOptions, WindowTimerMode};

async fn exercise(hard: bool) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let work = if hard {
        "while(true){}"
    } else {
        "const end=performance.now()+100;while(performance.now()<end){}"
    };
    let init = format!("if(location.pathname==='/next'){{const marker=document.querySelector('#destination');console.log('aux-callback-entered');marker.dataset.entered='1';marker.dataset.active='true';try{{{work}}}finally{{marker.dataset.active='false';marker.dataset.exited='1';}}}}");
    let origin = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut request = vec![];
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(140)).await;
                let body = "<!doctype html><p id=destination>New document</p>";
                let kind = "text/html";
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let mut page = PageSession::open_with_document_response_and_options(
        "<!doctype html><p>Before navigation</p>",
        &origin,
        "",
        "",
        200,
        &init,
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        !page.hydration_diagnostics().budget_exceeded,
        "fixture opening must finish before navigation: {:?}",
        page.hydration_diagnostics()
    );
    assert!(page.hydration_diagnostics().open_error.is_none());
    let target = page.auxiliary_root_id().unwrap();
    let result = page
        .navigate_target_with_options(
            target,
            &format!("{origin}next"),
            "push",
            r#"{"timeoutMs":1000,"waitUntil":"load"}"#,
        )
        .await;
    let events = page.network_event_log().drain();
    assert!(
        events.contains("aux-callback-entered"),
        "actual callback must enter: {result:?}"
    );
    if hard {
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .starts_with("Execution interrupted by the session execution budget"),
            "{result:?}"
        );
        page.close();
        server.abort();
        return;
    }
    let state: Value = serde_json::from_str(&page.read_target_engine(target,
        "return {url:location.href, destination:!!document.querySelector('#destination'),turn:document.querySelector('#destination')?.dataset};").unwrap()).unwrap();
    assert_eq!(
        state["destination"], true,
        "actual replacement document must be selected"
    );
    assert_eq!(
        state["turn"]["entered"], "1",
        "real required script must execute"
    );
    assert_eq!(
        state["turn"]["active"], "false",
        "healthy callback stack was interrupted"
    );
    assert_eq!(state["turn"]["exited"], "1");
    result.unwrap();
    page.close();
    server.abort();
}
#[tokio::test(flavor = "current_thread")]
async fn navigation_io_does_not_consume_healthy_callback_execution_budget() {
    exercise(false).await
}
#[tokio::test(flavor = "current_thread")]
async fn navigation_infinite_callback_is_still_interrupted() {
    exercise(true).await
}
