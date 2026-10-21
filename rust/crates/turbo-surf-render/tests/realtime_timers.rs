//! Opt-in browser comparison timing leaves the crawler's virtual scheduler intact.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::{
    BrowserStorage, DocumentResponseMetadata, PageSession, PageSessionOptions, WindowTimerMode,
};

async fn realtime(html: &str, base: &str, budget: u64) -> PageSession {
    PageSession::open_with_document_response_and_options(
        html,
        base,
        "",
        "",
        budget,
        "",
        ClientHints::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

async fn read_request_headers(socket: &mut tokio::net::TcpStream) {
    let mut request = Vec::new();
    loop {
        let mut chunk = [0; 1024];
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0, "request ended before its headers");
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            return;
        }
        assert!(request.len() < 65536, "unexpectedly large request headers");
    }
}

async fn value(page: &mut PageSession, script: &str) -> Value {
    serde_json::from_str(&page.eval_json_expression(script).await.unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn realtime_hydration_and_manual_drains_leave_future_timers_queued() {
    let mut page = realtime(
        "<body><script>globalThis.ticks=0;globalThis.once=false;setInterval(()=>ticks++,60000);setTimeout(()=>once=true,60000);</script></body>",
        "https://timer.test/", 500,
    ).await;
    assert!(!page.hydration_diagnostics().budget_exceeded);
    page.eval("globalThis[Symbol.for('ts')].runTimers(2000);globalThis.__RESULT='done';")
        .await
        .unwrap();
    assert_eq!(value(&mut page, "[ticks,once]").await, json!([0, false]));
    assert!(!page.hydration_diagnostics().budget_exceeded);
}

#[tokio::test(flavor = "current_thread")]
async fn awaited_realtime_timer_advances_guest_and_host_time_once() {
    let mut page = realtime("<body></body>", "https://timer.test/", 2000).await;
    let started = Instant::now();
    let observed = value(&mut page, "new Promise(resolve=>{const p=performance.now(),d=Date.now();setTimeout(()=>resolve([performance.now()-p,Date.now()-d]),120);})").await;
    let wall = started.elapsed().as_secs_f64() * 1000.0;
    let guest = observed[0].as_f64().unwrap();
    assert!(wall >= 120.0, "host {wall}ms, guest {observed}");
    assert!(
        guest >= 119.0 && guest <= wall + 1.0,
        "host {wall}ms, guest {observed}"
    );
    assert!(
        (guest - observed[1].as_f64().unwrap()).abs() < 2.0,
        "{observed}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn due_timer_microtasks_and_native_io_share_realtime_ordering() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request_headers(&mut socket).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await.unwrap();
    });
    let mut page = realtime("<body></body>", &base, 3000).await;
    let observed = value(&mut page, "(async()=>{const out=[],start=performance.now();await Promise.all([new Promise(resolve=>setTimeout(()=>{out.push(['timer',performance.now()-start]);Promise.resolve().then(()=>{out.push(['microtask',performance.now()-start]);resolve();});},80)),fetch('/slow').then(r=>r.text()).then(body=>out.push([body,performance.now()-start]))]);return out;})()").await;
    assert_eq!(observed[0][0], "timer");
    assert_eq!(observed[1][0], "microtask");
    assert_eq!(observed[2][0], "ok");
    assert!(observed[0][1].as_f64().unwrap() >= 79.0, "{observed}");
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn child_frames_and_parent_share_realtime_deadlines_without_future_leaps() {
    let mut page = realtime(
        "<body><iframe></iframe></body>",
        "https://timer.test/",
        2000,
    )
    .await;
    page.eval("globalThis.parentTick=false;const w=document.querySelector('iframe').contentWindow;w.childTick=false;setTimeout(()=>parentTick=true,60000);w.setTimeout(()=>w.childTick=true,60000);globalThis[Symbol.for('ts')].runTimers();").await.unwrap();
    assert_eq!(
        value(
            &mut page,
            "[parentTick,document.querySelector('iframe').contentWindow.childTick]"
        )
        .await,
        json!([false, false])
    );
    let started = Instant::now();
    assert_eq!(value(&mut page, "new Promise(resolve=>document.querySelector('iframe').contentWindow.setTimeout(()=>resolve('child'),100))").await, "child");
    assert!(started.elapsed() >= Duration::from_millis(100));
}

#[tokio::test(flavor = "current_thread")]
async fn realtime_wait_budget_cancels_pending_input_and_keeps_session_usable() {
    let mut page = realtime("<body><input id=q></body>", "https://timer.test/", 100).await;
    let started = Instant::now();
    let error = page
        .input(&json!([{"focus":"#q"},{"wait":250},{"type":"late"}]))
        .await
        .unwrap_err();
    assert!(error.contains("budget"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(2));
    tokio::time::sleep(Duration::from_millis(200)).await;
    page.eval("globalThis.__RESULT='pump after canceled deadline';")
        .await
        .unwrap();
    assert_eq!(
        value(&mut page, "document.querySelector('#q').value").await,
        ""
    );
    page.input(&json!([{"press":"Escape"}])).await.unwrap();
    assert_eq!(value(&mut page, "document.body.tagName").await, "BODY");
}

#[tokio::test(flavor = "current_thread")]
async fn realtime_initial_resource_wait_obeys_the_original_session_budget() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request_headers(&mut socket).await;
        accepted_tx.send(()).unwrap();
        std::future::pending::<()>().await;
        drop(socket);
    });
    let mut page = realtime("<script src='/never.js'></script>", &base, 100).await;
    assert!(page.hydration_diagnostics().budget_exceeded);
    tokio::time::timeout(Duration::from_secs(1), accepted_rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value(&mut page, "document.body.tagName").await, "BODY");
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn realtime_awaited_timer_obeys_the_original_session_budget() {
    let mut page = realtime("<body></body>", "https://timer.test/", 100).await;
    let started = Instant::now();
    let error = page
        .eval_json_expression("new Promise(resolve=>setTimeout(resolve,60000))")
        .await
        .unwrap_err();
    assert!(error.contains("budget"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(value(&mut page, "document.body.tagName").await, "BODY");
}

#[tokio::test(flavor = "current_thread")]
async fn default_constructor_retains_fast_virtual_timers() {
    let mut page = PageSession::open("<body></body>", "https://timer.test/", "", "", 2000)
        .await
        .unwrap();
    let started = Instant::now();
    let elapsed=value(&mut page,"new Promise(resolve=>{const start=performance.now();setTimeout(()=>resolve(performance.now()-start),1000)})").await;
    assert!(elapsed.as_f64().unwrap() >= 999.0, "{elapsed}");
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[tokio::test(flavor = "current_thread")]
async fn automatic_form_navigation_inherits_realtime_policy() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request_headers(&mut socket).await;
        let body="<body><script>globalThis.fired=false;setTimeout(()=>fired=true,60000);</script>destination</body>";
        let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    let mut page = realtime(
        "<form action='/destination'></form>",
        &format!("{base}/start"),
        2000,
    )
    .await;
    page.eval("document.querySelector('form').requestSubmit()")
        .await
        .unwrap();
    assert_eq!(page.navigation_snapshot().0, 1);
    assert_eq!(value(&mut page, "fired").await, false);
    assert!(!page.hydration_diagnostics().budget_exceeded);
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn initial_hydration_cap_does_not_shorten_later_operation_budgets() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request_headers(&mut socket).await;
        accepted_tx.send(()).unwrap();
        std::future::pending::<()>().await;
        drop(socket);
    });
    let mut page = PageSession::open_with_document_response_and_options(
        "<script src='/never.js'></script>",
        &base,
        "",
        "",
        2000,
        "",
        ClientHints::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            initial_hydration_budget_ms: Some(100),
        },
    )
    .await
    .unwrap();
    assert!(page.hydration_diagnostics().budget_exceeded);
    assert!(page
        .hydration_diagnostics()
        .open_error
        .as_ref()
        .unwrap()
        .contains("100ms"));
    tokio::time::timeout(Duration::from_secs(1), accepted_rx)
        .await
        .unwrap()
        .unwrap();
    let started = Instant::now();
    assert_eq!(
        value(
            &mut page,
            "new Promise(resolve=>setTimeout(()=>resolve('original operation budget'),200))"
        )
        .await,
        "original operation budget"
    );
    assert!(started.elapsed() >= Duration::from_millis(200));
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn huge_finite_delays_during_hydration_and_in_child_frames_do_not_panic() {
    let mut page = realtime("<body><iframe></iframe><script>globalThis.fired=false;setTimeout(()=>fired=true,1e300);</script></body>", "https://timer.test/", 2000).await;
    assert_eq!(value(&mut page, "fired").await, true);
    assert!(!page.hydration_diagnostics().budget_exceeded);
    assert_eq!(value(&mut page, "new Promise(resolve=>document.querySelector('iframe').contentWindow.setTimeout(()=>resolve('child huge delay'),1e300))").await, "child huge delay");
}

async fn check_saved_timer_conversion(page: &mut PageSession) {
    let probe = include_str!("../../../../harness/detection/fixtures/timer-conversion-probe.js");
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/timer-conversion-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        oracle["probeSha256"],
        format!("{:x}", Sha256::digest(probe.as_bytes()))
    );
    let encoded = value(page, probe).await;
    let observed: Value = serde_json::from_str(encoded.as_str().unwrap()).unwrap();
    assert_eq!(
        observed["immediateFired"],
        oracle["result"]["immediateFired"]
    );
    let rows = observed["rows"].as_array().unwrap();
    let expected_rows = oracle["result"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), expected_rows.len());
    for (actual, expected) in rows.iter().zip(expected_rows) {
        for field in ["kind", "label", "fired", "error"] {
            assert_eq!(actual[field], expected[field], "{field}: {actual}");
        }
        if actual["label"] == "uint32-wrap-five" {
            assert!(actual["elapsedMs"].as_f64().unwrap() >= 4.9, "{actual}");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn realtime_timeout_and_interval_delay_conversion_matches_saved_chrome() {
    let mut page = realtime("<body></body>", "https://timer.test/", 2000).await;
    check_saved_timer_conversion(&mut page).await;
}

#[tokio::test(flavor = "current_thread")]
async fn virtual_timeout_and_interval_delay_conversion_matches_saved_chrome() {
    let mut page = PageSession::open("<body></body>", "https://timer.test/", "", "", 2000)
        .await
        .unwrap();
    check_saved_timer_conversion(&mut page).await;
}

#[tokio::test(flavor = "current_thread")]
async fn realtime_initial_timers_preserve_full_microtask_checkpoints() {
    let mut page = realtime(
        r#"<body><script>
          globalThis.timerEvents=[];
          document.addEventListener('DOMContentLoaded',()=>{
            setTimeout(()=>{
              timerEvents.push('first');
              Promise.resolve().then(()=>Promise.resolve()).then(()=>timerEvents.push('deep microtask'));
            },0);
            setTimeout(()=>timerEvents.push('second'),0);
          });
        </script></body>"#,
        "https://timer.test/",
        2000,
    )
    .await;
    assert_eq!(
        value(&mut page, "timerEvents").await,
        json!(["first", "deep microtask", "second"])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn timer_cancellation_converts_fractional_and_wrapped_ids_to_webidl_long() {
    let mut page = realtime("<body>", "https://timer.test/", 2000).await;
    let observed = value(
        &mut page,
        r#"(async()=>{
          const out=[];
          for(const [clear,offset] of [[clearTimeout,0.75],[clearInterval,4294967296.75]]){
            let fired=false;
            const id=setTimeout(()=>fired=true,0);
            clear(id+offset);
            await new Promise(resolve=>setTimeout(resolve,20));
            out.push(fired);
          }
          return out;
        })()"#,
    )
    .await;
    assert_eq!(observed, json!([false, false]));
}

#[tokio::test(flavor = "current_thread")]
async fn timer_admission_does_not_call_guest_replacements_of_math_max() {
    let mut page = realtime("<body>", "https://timer.test/", 2000).await;
    let observed = value(
        &mut page,
        r#"new Promise(resolve=>{
          const original=Math.max;
          try{
            Math.max=()=>{throw new Error('guest Math.max called by timer admission');};
            setTimeout(()=>resolve('timer delivered'),0);
          }finally{Math.max=original;}
        })"#,
    )
    .await;
    assert_eq!(observed, "timer delivered");
}
