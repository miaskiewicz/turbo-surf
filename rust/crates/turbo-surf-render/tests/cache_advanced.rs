use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn resource_loaders_clear_site_data_and_ancestor_partition_match_chrome155() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let counts = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let wire_requests = requests.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let counts = counts.clone();
            let requests = wire_requests.clone();
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
                let date = httpdate::fmt_http_date(
                    std::time::SystemTime::now() - std::time::Duration::from_secs(60),
                );
                let special = match target.path() {
                    "/clear" => Some(("text/plain", "Cache-Control: no-store\r\n".to_string()+&format!("Clear-Site-Data: {}\r\n",query.get("types").map_or("\"cache\"",String::as_str)), b"cleared".to_vec())),
                    "/frame-direct" => Some(("text/html",String::new(),format!("<!doctype html><script>(async()=>{{const response=await fetch('http://127.0.0.1:{}/cache/nested');top.postMessage({{kind:'cache-ancestor',key:new URL(location.href).searchParams.get('key'),count:response.headers.get('x-count')}},'*')}})()</script>",socket.local_addr().unwrap().port()).into_bytes())),
                    "/frame-parent" => Some(("text/html",String::new(),format!("<!doctype html><iframe src=\"http://127.0.0.1:{}/frame-direct?key=nested\"></iframe>",socket.local_addr().unwrap().port()).into_bytes())),
                    "/module-frame" => Some(("text/html",String::new(),b"<!doctype html><script type=\"module\">import {value} from '/resource/module';top.postMessage({kind:'resource-module',key:new URL(location.href).searchParams.get('key'),value},'*')</script>".to_vec())),
                    "/resource/classic" => Some(("text/javascript","Cache-Control: max-age=3600\r\n".into(),format!("globalThis.resourceClassic=(globalThis.resourceClassic||[]);globalThis.resourceClassic.push({count});").into_bytes())),
                    "/resource/module" => Some(("text/javascript","Cache-Control: max-age=3600\r\n".into(),format!("export const value={count};").into_bytes())),
                    "/resource/style" => Some(("text/css","Cache-Control: max-age=3600\r\n".into(),format!("body{{margin-left:{count}px}}").into_bytes())),
                    "/resource/image" => Some(("image/gif","Cache-Control: max-age=3600\r\n".into(), {use base64::Engine;base64::engine::general_purpose::STANDARD.decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7").unwrap()})),
                    "/stats" => Some(("application/json","Cache-Control: no-store\r\n".into(),counts.lock().unwrap().get(query.get("path").unwrap()).copied().unwrap_or(0).to_string().into_bytes())),
                    _ => None,
                };
                if let Some((content, fields, body)) = special {
                    let mut response=format!("HTTP/1.1 200 OK\r\nContent-Type: {content}\r\n{fields}Date: {date}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
                    response.extend(body);
                    let _ = socket.write_all(&response).await;
                    return;
                }
                if target.path().starts_with("/range/") {
                    requests.lock().unwrap().push(serde_json::json!({
                        "url": path,
                        "range": headers.get("range"),
                        "ifRange": headers.get("if-range"),
                    }));
                    let bytes = b"abcdefghij";
                    let range = headers
                        .get("range")
                        .and_then(|value| value.strip_prefix("bytes="))
                        .and_then(|value| value.split_once('-'));
                    let mut status = "200 OK";
                    let mut range_field = String::new();
                    let mut body = bytes.to_vec();
                    if let Some((start, end)) = range {
                        let start = if start.is_empty() {
                            10usize.saturating_sub(end.parse().unwrap())
                        } else {
                            start.parse::<usize>().unwrap()
                        };
                        let end = if end.is_empty() {
                            9
                        } else {
                            end.parse::<usize>().unwrap().min(9)
                        };
                        if start > end || start >= 10 {
                            status = "416 Range Not Satisfiable";
                            range_field = "Content-Range: bytes */10\r\n".into();
                            body.clear();
                        } else {
                            status = "206 Partial Content";
                            range_field = format!("Content-Range: bytes {start}-{end}/10\r\n");
                            body = bytes[start..=end].to_vec();
                        }
                    }
                    let mut response=format!("HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nCache-Control: max-age=3600\r\nETag: \"range\"\r\nAccept-Ranges: bytes\r\nX-Count: {count}\r\n{range_field}Date: {date}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
                    response.extend(body);
                    let _ = socket.write_all(&response).await;
                    return;
                }
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
        60_000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/cache-advanced-probe.js")
        .trim()
        .trim_end_matches(';');
    // V8 150 has no child-realm module loader; the 155 gate checks that source too.
    #[cfg(not(turbo_surf_native_proxy))]
    let probe = probe.replace(
        "result.modules = [await moduleFrame(\"first\"), await moduleFrame(\"second\")];",
        "",
    );
    let actual = page
        .eval(&format!(
            "(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})()"
        ))
        .await;
    if let Err(error) = &actual {
        let diagnostic=page.eval_expression("JSON.stringify({stage:globalThis.__cacheStage,classic:globalThis.resourceClassic,frames:[...document.querySelectorAll('iframe')].map(frame=>({src:frame.src,href:frame.contentWindow.location.href,body:frame.contentDocument?.body?.textContent}))})").await;
        panic!("{error}; frame diagnostic: {diagnostic:?}");
    }
    let actual = actual.unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/cache-advanced-chrome155.json"
    ))
    .unwrap();
    for (key, expected) in expected["result"].as_object().unwrap() {
        #[cfg(not(turbo_surf_native_proxy))]
        if key == "modules" {
            continue;
        }
        assert_eq!(&actual[key], expected, "cache case {key}");
    }
    assert_eq!(
        serde_json::json!(*requests.lock().unwrap()),
        expected["requests"]
    );
    server.abort();
}
