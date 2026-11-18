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
async fn host_route_fetch_held_binary_is_hidden_while_root_request_releases_real_transport() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut upstream = accept(&listener).await;
        assert_eq!(path(&mut upstream).await, "/upstream-complete");
        upstream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n\x00\xffA").await.unwrap();
        // The second real request is the admission witness and releases the held body.
        let mut release = accept(&listener).await;
        assert_eq!(path(&mut release).await, "/release");
        release
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nreleased")
            .await
            .unwrap();
        upstream.write_all(b"\x00\x80Z").await.unwrap();
        upstream.shutdown().await.unwrap();
    });
    let mut page = PageSession::open("<body><p id=stable>root</p>", &base, "", "", 5000)
        .await
        .unwrap();
    let result = page.eval_engine_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const realm=__tsNs.shimBindingNamespace(child);
      const response=await realm.shimRouteFetch(new URL('/upstream-complete',location.href).href);
      const reading=response.arrayBuffer();
      const release=await fetch('/release').then(value=>value.text());
      const buffer=await reading;
      const value={status:response.status,bytes:Array.from(new Uint8Array(buffer)),childBuffer:buffer instanceof child.ArrayBuffer&&!(buffer instanceof ArrayBuffer),release,stable:document.querySelector('#stable').textContent};
      frame.remove();return value;
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({"status":200,"bytes":[0,255,65,0,128,90],"childBuffer":true,"release":"released","stable":"root"})
    );
    server.await.unwrap();
    only_visible_response(&mut page, "/release", "released");
    assert_eq!(
        page.eval_json_expression("[typeof __tsNs,typeof Deno]")
            .await
            .unwrap(),
        "[\"undefined\",\"undefined\"]"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn host_route_fetch_truncated_body_rejects_without_exposing_upstream_or_poisoning_root() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut upstream = accept(&listener).await;
        assert_eq!(path(&mut upstream).await, "/upstream-truncated");
        upstream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n\x00\xffA").await.unwrap();
        upstream.shutdown().await.unwrap();
        drop(upstream);
        let mut healthy = accept(&listener).await;
        assert_eq!(path(&mut healthy).await, "/healthy");
        healthy
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nhealthy")
            .await
            .unwrap();
    });
    let mut page = PageSession::open("<body><p id=stable>root</p>", &base, "", "", 5000)
        .await
        .unwrap();
    let result = page.eval_engine_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const realm=__tsNs.shimBindingNamespace(child);
      const response=await realm.shimRouteFetch(new URL('/upstream-truncated',location.href).href);
      let rejected=false,childError=false,privateError=false;
      try{await response.arrayBuffer();}catch(error){rejected=error.name==='TypeError';childError=error instanceof child.TypeError&&!(error instanceof TypeError);privateError=error.constructor.constructor('return typeof Deno')()==='undefined';}
      frame.remove();
      const healthy=await fetch('/healthy').then(value=>value.text());
      return {status:response.status,rejected,childError,privateError,healthy,stable:document.querySelector('#stable').textContent};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({"status":200,"rejected":true,"childError":true,"privateError":true,"healthy":"healthy","stable":"root"})
    );
    server.await.unwrap();
    only_visible_response(&mut page, "/healthy", "healthy");
    assert_eq!(
        page.eval_json_expression("[typeof __tsNs,typeof Deno]")
            .await
            .unwrap(),
        "[\"undefined\",\"undefined\"]"
    );
    page.close();
}
