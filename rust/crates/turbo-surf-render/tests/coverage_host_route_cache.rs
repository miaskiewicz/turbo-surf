//! The real host route-fetch entry keeps upstream transport private without hiding guest work.
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

async fn accept(listener: &tokio::net::TcpListener) -> tokio::net::TcpStream {
    tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
        .await
        .expect("real fixture request admitted within the page budget")
        .unwrap()
        .0
}

async fn path(socket: &mut tokio::net::TcpStream) -> String {
    let mut request = Vec::new();
    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
        let mut bytes = [0; 1024];
        let count = socket.read(&mut bytes).await.unwrap();
        assert!(count > 0, "real request headers must complete");
        request.extend_from_slice(&bytes[..count]);
        assert!(request.len() <= 16 * 1024);
    }
    String::from_utf8(request)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .into()
}

fn only_visible_response(page: &mut PageSession, expected_path: &str, body: &str) {
    let entries: Value = serde_json::from_str(&page.drain_network_log()).unwrap();
    let responses: Vec<_> = entries
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["event"] == "response")
        .collect();
    assert_eq!(
        responses.len(),
        1,
        "host upstream must not publish a response: {entries}"
    );
    let response = responses[0];
    assert_eq!(
        url::Url::parse(response["url"].as_str().unwrap())
            .unwrap()
            .path(),
        expected_path
    );
    assert_eq!(response["status"], 200);
    assert_eq!(response["body"], body);
    assert_eq!(response["bodyIncomplete"], false);
    assert_eq!(response["bodyBytes"], json!(body.as_bytes()));
    assert!(!entries.to_string().contains("/upstream"));
}

#[tokio::test(flavor = "current_thread")]
async fn host_route_cache_reuses_real_bytes_without_exposing_upstream_responses() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = requests.clone();
    let server = tokio::spawn(async move {
        loop {
            let mut socket = accept(&listener).await;
            let target = path(&mut socket).await;
            observed.lock().unwrap().push(target.clone());
            match target.as_str() {
                "/upstream-cache" => socket.write_all(b"HTTP/1.1 200 OK\r\nCache-Control: max-age=60\r\nContent-Length: 6\r\nConnection: close\r\n\r\n\x00\xffA\x00\x80Z").await.unwrap(),
                "/healthy" => {
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nhealthy").await.unwrap();
                    break;
                }
                _ => panic!("unexpected actual request: {target}"),
            }
        }
    });
    let mut page = PageSession::open("<body><p id=stable>root</p>", &base, "", "", 5000)
        .await
        .unwrap();
    let raw = page.eval_engine_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const realm=__tsNs.shimBindingNamespace(child),url=new URL('/upstream-cache',location.href).href;
      const first=await realm.shimRouteFetch(url);
      const copy=first.clone(),firstBytes=Array.from(new Uint8Array(await copy.arrayBuffer()));
      const unread=!first.bodyUsed;
      const second=await realm.shimRouteFetch(url);
      const secondBytes=Array.from(new Uint8Array(await second.arrayBuffer()));
      const originalBytes=Array.from(new Uint8Array(await first.arrayBuffer()));
      frame.remove();const healthy=await fetch('/healthy').then(value=>value.text());
      return {statuses:[first.status,second.status],firstBytes,secondBytes,originalBytes,unread,healthy,stable:document.querySelector('#stable').textContent};
    })()"#).await.unwrap();
    let bytes = json!([0, 255, 65, 0, 128, 90]);
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"statuses":[200,200],"firstBytes":bytes,"secondBytes":bytes,"originalBytes":bytes,"unread":true,"healthy":"healthy","stable":"root"})
    );
    server.await.unwrap();
    assert_eq!(*requests.lock().unwrap(), ["/upstream-cache", "/healthy"]);
    only_visible_response(&mut page, "/healthy", "healthy");
    assert_eq!(
        page.eval_json_expression("[typeof __tsNs,typeof Deno]")
            .await
            .unwrap(),
        "[\"undefined\",\"undefined\"]"
    );
    page.close();
}
