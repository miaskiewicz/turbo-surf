//! Local resource and lifecycle contracts, shared with the Chrome oracle fixture.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_render::PageSession;

const FIXTURE: &str = include_str!("../../../../harness/detection/fixtures/lifecycle.html");
const PIXEL: &[u8] = include_bytes!("../../../../harness/detection/fixtures/pixel.png");

#[tokio::test(flavor = "current_thread")]
async fn session_runs_deferred_scripts_at_interactive_and_waits_for_decoded_resources_after_dcl() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    let resource_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            let resource_gate = resource_gate.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let headers: BTreeMap<String, String> = text
                    .lines()
                    .skip(1)
                    .filter_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        Some((name.to_ascii_lowercase(), value.trim().to_string()))
                    })
                    .collect();
                log.lock().unwrap().push((path.to_string(), headers));
                if path == "/resources-ready" {
                    resource_gate.add_permits(3);
                }
                if path.ends_with(".png") {
                    resource_gate.acquire().await.unwrap().forget();
                }
                let (mime, body, delay) = match path {
                    "/main.css" => (
                        "text/css",
                        b"@import '/import.css'; sentinel { color: blue; }".as_slice(),
                        0,
                    ),
                    "/import.css" => ("text/css", b"em { color: red; }".as_slice(), 0),
                    "/dynamic.css" => ("text/css", b"dynamic-sentinel {}".as_slice(), 0),
                    "/defer.js" => (
                        "application/javascript",
                        b"fixtureLog('defer:'+document.readyState);".as_slice(),
                        0,
                    ),
                    "/pixel.png" => ("image/png", PIXEL, 150),
                    "/broken.png" => ("image/png", b"not an image".as_slice(), 200),
                    "/dynamic.png" => ("image/png", PIXEL, 250),
                    _ => ("text/plain", b"missing".as_slice(), 0),
                };
                tokio::time::sleep(Duration::from_millis(delay)).await;
                let name = path.trim_start_matches('/').replace('.', "_");
                let deletion = if path == "/pixel.png" {
                    "Set-Cookie: stale=; Max-Age=0; Path=/\r\n"
                } else {
                    ""
                };
                let head=format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nSet-Cookie: {name}=loaded; Path=/\r\n{deletion}Connection: close\r\n\r\n",body.len());
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.write_all(body).await.unwrap();
            });
        }
    });
    let mut seed = CookieJar::new();
    seed.add("stale", "seed", "127.0.0.1", "/", None);
    let html = FIXTURE.replace(
        "fixtureLog('body:'+document.readyState);",
        "fixtureLog('body:'+document.readyState); globalThis.seedCookiePresent=document.cookie.includes('stale=seed'); document.cookie='script=written; Path=/';",
    );
    let init = "globalThis.hostCssReads=0;const descriptor=Object.getOwnPropertyDescriptor(CSSStyleSheet.prototype,'cssRules');Object.defineProperty(CSSStyleSheet.prototype,'cssRules',{...descriptor,get(){hostCssReads++;return descriptor.get.call(this);}});";
    let mut page = PageSession::open_with_init_script(
        &html,
        &format!("{base}/"),
        &seed.storage_state(),
        "",
        5000,
        init,
    )
    .await
    .unwrap();
    let raw = page
        .eval("globalThis.__RESULT=JSON.stringify({...__browserFixture,seedCookiePresent,hostCssReads,resources:performance.getEntriesByType('resource').map(entry=>({name:entry.name,initiatorType:entry.initiatorType,startTime:entry.startTime,duration:entry.duration}))})")
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(result["seedCookiePresent"], true, "{result}");
    assert_eq!(
        result["hostCssReads"], 4,
        "host stylesheet loading must bypass target-installed CSSOM instrumentation: {result}"
    );
    let cookies = CookieJar::from_storage_state(&page.cookies()).cookie_header(&base, 0.0);
    assert!(cookies.contains("script=written"), "{cookies}");
    assert!(!cookies.contains("stale="), "{cookies}");
    for name in [
        "pixel_png",
        "dynamic_png",
        "broken_png",
        "main_css",
        "import_css",
        "dynamic_css",
        "defer_js",
    ] {
        assert!(cookies.contains(&format!("{name}=loaded")), "{cookies}");
    }
    page.close();
    server.abort();
    assert_eq!(
        result["events"],
        serde_json::json!([
            "classic:loading",
            "body:loading",
            "state:interactive",
            "defer:interactive",
            "dcl:interactive",
            "image:load",
            "image:error",
            "state:complete",
            "load:complete",
            "pageshow"
        ]),
        "{result}"
    );
    assert_eq!(
        result["imageAtDcl"], false,
        "images must not delay DOMContentLoaded: {result}"
    );
    assert_eq!(
        result["image"],
        serde_json::json!({"complete":true,"width":3,"height":2})
    );
    assert_eq!(
        result["broken"],
        serde_json::json!({"complete":true,"width":0})
    );
    assert_eq!(result["linked"]["href"], format!("{base}/main.css"));
    assert_eq!(
        result["linked"]["rules"],
        serde_json::json!(["import", "sentinel"])
    );
    assert_eq!(result["linked"]["imported"], "em");
    assert_eq!(
        result["dynamic"],
        serde_json::json!({"imageWidth":3,"styleRule":"dynamic-sentinel"})
    );
    assert_eq!(result["control"], "parsed-token");
    let resources = result["resources"].as_array().unwrap();
    for (path, initiator) in [
        ("/main.css", "link"),
        ("/import.css", "css"),
        ("/dynamic.css", "link"),
        ("/pixel.png", "img"),
        ("/broken.png", "img"),
        ("/dynamic.png", "img"),
        ("/defer.js", "script"),
    ] {
        let url = format!("{base}{path}");
        let entries: Vec<_> = resources
            .iter()
            .filter(|entry| entry["name"] == url)
            .collect();
        assert_eq!(entries.len(), 1, "{path}: {resources:?}");
        assert_eq!(entries[0]["initiatorType"], initiator);
        assert!(entries[0]["startTime"].as_f64().unwrap() >= 0.0);
        let duration = entries[0]["duration"].as_f64().unwrap();
        assert!(duration >= 0.0 && duration.is_finite());
        if path.ends_with(".png") {
            assert!(duration >= 100.0, "actual gated image duration: {duration}");
        }
    }
    let host_events = result["hostEvents"].as_array().unwrap();
    assert_eq!(host_events.len(), 5, "{result}");
    for event in host_events {
        assert_eq!(event["trusted"], true, "{event}");
        assert_eq!(event["phase"], 2, "{event}");
        assert_eq!(event["timestampValid"], true, "{event}");
    }
    let completed_events = result["completedEventStates"].as_array().unwrap();
    assert_eq!(completed_events.len(), 5, "{result}");
    for (index, event) in completed_events.iter().enumerate() {
        assert_eq!(event["trusted"], true, "{event}");
        if index < 3 {
            assert!(event["currentTarget"].is_null(), "{event}");
            assert_eq!(event["phase"], 0, "{event}");
        } else {
            assert_eq!(event["currentTarget"], "window", "{event}");
            assert_eq!(event["phase"], 2, "{event}");
            assert_eq!(event["pathLength"], 1, "{event}");
        }
    }
    for event in result["redispatchStates"].as_array().unwrap() {
        assert_eq!(event["error"], "InvalidStateError", "{event}");
        assert_eq!(event["trusted"], true, "{event}");
    }
    let requests = requests.lock().unwrap();
    for (path, destination) in [
        ("/pixel.png", "image"),
        ("/dynamic.png", "image"),
        ("/broken.png", "image"),
        ("/main.css", "style"),
        ("/import.css", "style"),
        ("/dynamic.css", "style"),
        ("/defer.js", "script"),
    ] {
        let rows: Vec<_> = requests.iter().filter(|(url, _)| url == path).collect();
        assert_eq!(
            rows.len(),
            1,
            "each resource should be requested once: {path} {requests:?}"
        );
        let headers = &rows[0].1;
        assert_eq!(
            headers["sec-fetch-dest"], destination,
            "{path}: {headers:?}"
        );
        assert_eq!(headers["sec-fetch-mode"], "no-cors");
        assert!(!headers.contains_key("upgrade-insecure-requests"));
        assert!(!headers.contains_key("sec-fetch-user"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn inline_scripts_have_no_element_load_event_and_load_handlers_can_hydrate() {
    let html = r#"<body><script id="inline">globalThis.inlineLoads=0;document.querySelector('#inline').addEventListener('load',()=>inlineLoads++);window.addEventListener('load',()=>{const script=document.createElement('script');script.textContent='globalThis.afterLoad=42';document.body.appendChild(script);});</script></body>"#;
    let mut page = PageSession::open(html, "https://example.test/", "", "", 5000)
        .await
        .unwrap();
    let raw=page.eval("globalThis.__RESULT=JSON.stringify({loads:inlineLoads,value:globalThis.afterLoad,state:document.readyState})").await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&raw).unwrap(),
        serde_json::json!({"loads":0,"value":42,"state":"complete"})
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn defer_on_inline_classic_scripts_does_not_reorder_them() {
    let html = r#"<script defer>globalThis.order=['first:'+document.readyState]</script><script>order.push('second:'+document.readyState)</script>"#;
    let mut page = PageSession::open(html, "https://example.test/", "", "", 5000)
        .await
        .unwrap();
    let value = page
        .eval("globalThis.__RESULT=JSON.stringify(order)")
        .await
        .unwrap();
    assert_eq!(value, r#"["first:loading","second:loading"]"#);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn asynchronous_classic_scripts_do_not_block_dcl_but_delay_load_with_native_events() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let gate = gate.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let body = match path {
                    "/async.js" => {
                        gate.acquire().await.unwrap().forget();
                        "scriptOrder.push('async:'+document.readyState);window.asyncReady=true;"
                    }
                    "/defer.js" => "scriptOrder.push('defer:'+document.readyState);",
                    "/ready" => {
                        gate.add_permits(1);
                        "ready"
                    }
                    _ => "missing",
                };
                let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    let fixture = include_str!("../../../../harness/detection/fixtures/script-lifecycle.html");
    let mut page = tokio::time::timeout(
        Duration::from_secs(5),
        PageSession::open(fixture, &base, "[]", "", 5000),
    )
    .await
    .unwrap()
    .unwrap();
    let raw = page
        .eval("globalThis.__RESULT=document.body.getAttribute('data-browser-fixture')")
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        result["order"],
        serde_json::json!([
            "inline:loading",
            "body:loading",
            "defer:interactive",
            "dcl:interactive",
            "async:interactive",
            "load:complete"
        ]),
        "{result}"
    );
    assert_eq!(result["asyncReady"], true);
    assert_eq!(
        result["events"],
        serde_json::json!([{"type":"load","trusted":true,"event":true,"target":true}])
    );
    page.close();
    server.abort();
}
