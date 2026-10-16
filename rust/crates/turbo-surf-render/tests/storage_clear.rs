use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn clear_site_data_storage_matches_source_pinned_chrome() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let secondary = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_port = secondary.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        loop {
            let accepted = tokio::select! { accepted=listener.accept()=>accepted, accepted=secondary.accept()=>accepted };
            let Ok((mut socket, _)) = accepted else {
                break;
            };
            tokio::spawn(async move {
                let mut bytes = [0; 8192];
                let size = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..size]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                let url = url::Url::parse(&format!("http://127.0.0.1:{port}{path}")).unwrap();
                let query = url
                    .query_pairs()
                    .collect::<std::collections::BTreeMap<_, _>>();
                let mut headers = String::new();
                if url.path() != "/resource-clear" {
                    headers.push_str("Content-Type: text/html\r\n");
                }
                if query.contains_key("clearstorage") {
                    headers.push_str("Clear-Site-Data: \"storage\"\r\n");
                }
                let body = if url.path() == "/widget" {
                    format!(
                        "<!doctype html><body><script>{}</script>",
                        include_str!("../../../../harness/detection/storage-clear-widget.js")
                    )
                } else if url.path() == "/resource-clear" {
                    headers.push_str("Clear-Site-Data: \"storage\"\r\n");
                    match query.get("kind").unwrap().as_ref() {
                        "style" => {
                            headers.push_str("Content-Type: text/css\r\n");
                            "body { color: red; }".into()
                        }
                        "image" => {
                            headers.push_str("Content-Type: image/svg+xml\r\n");
                            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"></svg>".into()
                        }
                        _ => {
                            headers.push_str("Content-Type: text/javascript\r\n");
                            "globalThis.__resourceClear = true;".into()
                        }
                    }
                } else if url.path() == "/clear" {
                    headers.push_str(&format!(
                        "Clear-Site-Data: {}\r\nCache-Control: max-age=60\r\n",
                        query.get("header").unwrap()
                    ));
                    if query.get("cors").is_some_and(|value| value == "allow") {
                        let origin = request
                            .lines()
                            .find_map(|line| {
                                line.split_once(':')
                                    .filter(|(name, _)| name.eq_ignore_ascii_case("origin"))
                                    .map(|(_, value)| value.trim())
                            })
                            .unwrap();
                        headers.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Credentials: true\r\n"));
                    }
                    if let Some(corp) = query.get("corp") {
                        headers.push_str(&format!("Cross-Origin-Resource-Policy: {corp}\r\n"));
                    }
                    "clear".into()
                } else {
                    "<!doctype html><body>".into()
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let mut page = PageSession::open(
        "<!doctype html><body>",
        &format!("http://127.0.0.1:{port}/"),
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    page.eval_expression(&format!(
        "globalThis.__storageClearSecondPort={second_port}"
    ))
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/storage-clear-probe.js")
        .trim()
        .trim_end_matches(';');
    let result = page.eval_json_expression(probe).await;
    if let Err(error) = &result {
        let diagnostic=page.eval_expression("JSON.stringify([...document.querySelectorAll('script,link,img')].map(el=>({tag:el.tagName,src:el.getAttribute('src'),href:el.getAttribute('href'),outer:el.outerHTML})))").await;
        panic!(
            "{error}; diagnostic={diagnostic:?}; network={}",
            page.drain_network_log()
        );
    }
    let result = result.unwrap();
    server.abort();
    let actual: serde_json::Value = serde_json::from_str(&result).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/storage-clear-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}
