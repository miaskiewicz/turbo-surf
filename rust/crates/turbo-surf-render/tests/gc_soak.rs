//! Soak controls for turbo-surf#115/#183. One render session creates and drops
//! many iframes with blob URLs, ports and detached subtrees, then forces a GC
//! boundary and checks that host-side tables return to their baseline. The GC
//! guest APIs are intentionally absent; the host drives collection through the
//! existing `collect_message_port_garbage_for_testing` boundary.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

/// A localhost page server: every path answers a small HTML document.
async fn serve() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut chunk = [0; 4096];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let size = socket.read(&mut chunk).await.unwrap_or(0);
                    if size == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..size]);
                }
                let html = "<!doctype html><html><head></head><body><p>served</p></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                    html.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    port
}

fn collect(page: &mut PageSession) {
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
}

/// After many data-URL iframes with blob URLs, ports and detached subtrees are
/// created and removed, host-side tables fall back to their initial baseline.
#[tokio::test(flavor = "current_thread")]
async fn host_tables_return_to_baseline_after_many_iframes_are_created_and_removed() {
    let mut page = PageSession::open("<body>", "https://soak.test/", "", "", 120_000)
        .await
        .unwrap();
    // A baseline captured after the session has settled. Early frames carry
    // one-time about:blank contexts; compare post-GC counts to the same state.
    collect(&mut page);
    let baseline_ports = page.message_port_diagnostics_for_testing();
    let baseline_lifecycle = page.frame_lifecycle_diagnostics_for_testing();
    let baseline_blobs = page.blob_url_store_size_for_testing();
    page.eval_expression(
        r##"(async()=>{
            globalThis.__soakFrames=[];
            for(let i=0;i<40;i++){
              const frame=document.createElement('iframe');
              const loaded=new Promise(resolve=>frame.addEventListener('load',resolve,{once:true}));
              const inner='<body><p>soak-'+i+'</p>'
                +'<script>'
                +'const url=URL.createObjectURL(new Blob([new Uint8Array(1024)],{type:"text/plain"}));'
                +'const ch=new MessageChannel();ch.port1.postMessage({payload:new ArrayBuffer(512)});'
                +'const detached=document.createElement("div");'
                +'for(let k=0;k<64;k++){const row=document.createElement("p");row.textContent="detached-"+k;detached.append(row);}'
                +'window.__retained={url,ch,detached};'
                +'<\/script>';
              frame.src='data:text/html;charset=utf-8,'+encodeURIComponent(inner);
              document.body.append(frame);
              await loaded;
              __soakFrames.push(frame);
              frame.remove();
              __soakFrames.pop();
            }
            return 'drained';
        })()"##,
    )
    .await
    .unwrap();
    collect(&mut page);
    let after_ports = page.message_port_diagnostics_for_testing();
    let after_lifecycle = page.frame_lifecycle_diagnostics_for_testing();
    assert_eq!(
        after_ports["endpoints"], baseline_ports["endpoints"],
        "message port endpoints: {baseline_ports} -> {after_ports}"
    );
    assert_eq!(
        after_ports["queuedBytes"], baseline_ports["queuedBytes"],
        "queued bytes: {baseline_ports} -> {after_ports}"
    );
    assert_eq!(
        after_ports["reservedBytes"], baseline_ports["reservedBytes"],
        "reserved bytes: {baseline_ports} -> {after_ports}"
    );
    assert_eq!(
        after_lifecycle["hostContexts"], baseline_lifecycle["hostContexts"],
        "host contexts: {baseline_lifecycle} -> {after_lifecycle}"
    );
    assert_eq!(
        after_lifecycle["queuedWindowMessages"], 0,
        "window message queue leaks: {after_lifecycle}"
    );
    let after_blobs = page.blob_url_store_size_for_testing();
    assert_eq!(
        after_blobs, baseline_blobs,
        "blob URL entries leaked: {baseline_blobs} -> {after_blobs}"
    );
}

/// `#183`: in one session, 60 same-origin iframe loads do not slow to seconds.
/// The soak is a reproduction on an idle localhost: a leak or teardown problem
/// would show the last loads running an order of magnitude slower than the
/// steady state. The budget is deliberately generous for shared-machine noise.
#[tokio::test(flavor = "current_thread")]
async fn localhost_frame_loads_stay_bounded_across_sixty_iterations() {
    let port = serve().await;
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("http://127.0.0.1:{port}/"),
        "",
        "",
        600_000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(
            "(async()=>{const times=[];for(let i=0;i<60;i++){const t=performance.now();const f=document.createElement('iframe');const l=new Promise(r=>f.addEventListener('load',r,{once:true}));f.src='/frame-'+i;document.body.append(f);await l;f.contentDocument.body.textContent;f.remove();times.push(Math.round(performance.now()-t));}return times;})()",
        )
        .await
        .unwrap();
    let times: Vec<u32> = serde_json::from_str(&raw).unwrap();
    // On an idle machine the tail runs in a few hundred ms; a 7-14 s leak would
    // show as the issue reports. The budget covers shared-machine noise without
    // being so slack that a real regression slips through, and the trend check
    // catches a linear slowdown that noise alone will not produce.
    let tail = &times[10..];
    let worst = tail.iter().copied().max().unwrap_or(0);
    assert!(
        worst < 7_000,
        "a tail iteration ran for {worst} ms, suggesting a frame teardown leak: {times:?}"
    );
    let first_half = tail[..tail.len() / 2].iter().copied().sum::<u32>();
    let second_half = tail[tail.len() / 2..].iter().copied().sum::<u32>();
    assert!(
        second_half < first_half * 3,
        "second half {second_half} ms grew far beyond first half {first_half} ms: {times:?}"
    );
}
