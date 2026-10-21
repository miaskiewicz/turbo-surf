use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn request_cache_modes_validators_vary_and_referrer_match_chrome155() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let counts = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let counts = counts.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let count = socket.read(&mut bytes).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&bytes[..count]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&request);
                let path = head.split_whitespace().nth(1).unwrap();
                let headers: BTreeMap<String, String> = head
                    .lines()
                    .skip(1)
                    .filter_map(|line| line.split_once(':'))
                    .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().into()))
                    .collect();
                let target = url::Url::parse(&format!("http://localhost{path}")).unwrap();
                let query: BTreeMap<_, _> = target.query_pairs().into_owned().collect();
                let count = {
                    let mut counts = counts.lock().unwrap();
                    let count = counts.entry(path.into()).or_default();
                    *count += 1;
                    *count
                };
                let mut fields = format!("Content-Type: application/json\r\nCache-Control: {}\r\nETag: \"stable\"\r\nX-Count: {count}\r\nX-Merged: {}\r\nDate: {}\r\n",headers.get("x-response-control").or_else(||query.get("control")).map_or("max-age=3600",String::as_str), if count==1 {"initial"}else{"validated"}, httpdate::fmt_http_date(std::time::SystemTime::now()-std::time::Duration::from_secs(60)));
                if let Some(vary) = query.get("vary") {
                    fields.push_str(&format!("Vary: {vary}\r\n"));
                }
                let (status, body) = if query.contains_key("redirect") {
                    fields.push_str("Location: /cache/redirect-target\r\n");
                    (
                        if query.get("redirect").is_some_and(|value| value == "302") {
                            "302 Found"
                        } else if query.get("redirect").is_some_and(|value| value == "307") {
                            "307 Temporary Redirect"
                        } else {
                            "301 Moved Permanently"
                        },
                        Vec::new(),
                    )
                } else if headers
                    .get("if-none-match")
                    .is_some_and(|value| value == "\"stable\"")
                {
                    ("304 Not Modified", Vec::new())
                } else {
                    (match query.get("status").map(String::as_str) {Some("201")=>"201 Created",Some("202")=>"202 Accepted",Some("302")=>"302 Found",Some("307")=>"307 Temporary Redirect",Some("404")=>"404 Not Found",Some("500")=>"500 Internal Server Error",_=>"200 OK"}, serde_json::to_vec(&serde_json::json!({"count":count,"conditional":headers.get("if-none-match"),"cacheControl":headers.get("cache-control"),"pragma":headers.get("pragma"),"vary":headers.get("x-flavor"),"referer":headers.contains_key("referer")})).unwrap())
                };
                let body = if head.starts_with("HEAD ") {
                    Vec::new()
                } else {
                    body
                };
                let mut response = format!(
                    "HTTP/1.1 {status}\r\n{fields}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes();
                response.extend_from_slice(&body);
                let _ = socket.write_all(&response).await;
            });
        }
    });
    let mut page = PageSession::open(
        "<!doctype html><body>HTTP cache control</body>",
        &format!("{origin}/"),
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/http-cache-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual = page
        .eval(&format!(
            "(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})()"
        ))
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/http-cache-chrome155.json"
    ))
    .unwrap();
    for (key, expected) in expected["result"].as_object().unwrap() {
        assert_eq!(&actual[key], expected, "cache case {key}");
    }
    server.abort();
}
