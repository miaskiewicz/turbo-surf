//! Fresh normal-Chrome controls and native response admission share authored cases.
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_render::PageSession;

async fn run_authored_fetch_control(probe: &str, control: &str) {
    let source = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cross = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source_url = format!("http://{}", source.local_addr().unwrap());
    let cross_url = format!("http://{}", cross.local_addr().unwrap());
    let definitions: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/coep-cases.json"
    ))
    .unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    let cases = definitions.clone();
    let origin = source_url.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = cross.accept().await {
            let origin = origin.clone();
            let log = log.clone();
            let cases = cases.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.windows(4).any(|row| row == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap();
                let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
                let params: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                let item = cases["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["id"] == params["case"])
                    .unwrap();
                let cookie = request
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                    .map(|(_, value)| value.trim())
                    .unwrap_or("");
                log.lock().unwrap().push(
                    json!({"id":format!("{}:{}", params["case"],params["mode"]),"cookie":cookie}),
                );
                let mut extra = String::new();
                for header in item["headers"].as_array().unwrap() {
                    extra.push_str(&format!(
                        "{}: {}\r\n",
                        header[0].as_str().unwrap(),
                        header[1].as_str().unwrap()
                    ));
                }
                let body = "authored COEP resource";
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nAccess-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Credentials: true\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let control: Value = serde_json::from_str(control).unwrap();
    let mut seed = CookieJar::new();
    seed.set_from_response(&source_url, &["seed=local; HttpOnly; Path=/".into()], 0.0);
    for expected in control["result"].as_array().unwrap() {
        let policy = expected["policy"].as_str().unwrap();
        requests.lock().unwrap().clear();
        let headers = if policy == "unsafe-none" {
            Vec::new()
        } else {
            vec![("Cross-Origin-Embedder-Policy".into(), policy.into())]
        };
        let settings = json!({"cases":definitions["cases"],"cross":cross_url});
        let init = format!("globalThis.__coepPolicies={settings};");
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            &format!("{source_url}/page"),
            &seed.storage_state(),
            "",
            10000,
            &init,
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = probe.trim().trim_end_matches(';');
        let actual = page
            .eval(&format!(
                "(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})()"
            ))
            .await
            .unwrap();
        let actual: Value = serde_json::from_str(&actual).unwrap();
        assert_eq!(actual, expected["rows"], "{policy}");
        assert_eq!(
            json!(*requests.lock().unwrap()),
            expected["requests"],
            "actual network headers for {policy}"
        );
    }
    server.abort();
    drop(source);
}

#[tokio::test(flavor = "current_thread")]
async fn native_no_cors_embedder_policy_and_real_cookie_headers_match_chrome() {
    run_authored_fetch_control(
        include_str!("../../../../harness/detection/coep-probe.js"),
        include_str!("../../../../harness/detection/fixtures/coep-chrome155.json"),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn srcdoc_fetch_inherits_embedder_policy_and_preserves_child_error_realm() {
    run_authored_fetch_control(
        include_str!("../../../../harness/detection/coep-inherit-probe.js"),
        include_str!("../../../../harness/detection/fixtures/coep-inherit-chrome155.json"),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn native_embedder_policy_controls_dom_resources_opaque_sheets_and_integrity() {
    let source = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cross = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source_url = format!("http://{}", source.local_addr().unwrap());
    let cross_url = format!("http://{}", cross.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    let origin = source_url.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = cross.accept().await {
            let log = log.clone();
            let origin = origin.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.windows(4).any(|row| row == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap();
                let cookie = request
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                    .map(|(_, value)| value.trim())
                    .unwrap_or("");
                log.lock()
                    .unwrap()
                    .push(json!({"path":path,"cookie":cookie}));
                let (mime, body): (&str, &[u8]) = match path {
                    "/sheet.css" | "/integrity.css" => {
                        ("text/css", b"sentinel { color: rgb(12, 34, 56) }")
                    }
                    "/pixel.png" => (
                        "image/png",
                        include_bytes!("../../../../harness/detection/fixtures/pixel.png"),
                    ),
                    "/plain.js" => (
                        "text/javascript",
                        b"globalThis.coepPlain=(globalThis.coepPlain||0)+1",
                    ),
                    _ => (
                        "text/javascript",
                        b"globalThis.coepAuthorized=(globalThis.coepAuthorized||0)+1",
                    ),
                };
                let corp = if path == "/authorized.js" {
                    "Cross-Origin-Resource-Policy: cross-origin\r\n"
                } else {
                    ""
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nAccess-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Credentials: true\r\n{corp}Content-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
                socket.write_all(body).await.unwrap();
            });
        }
    });
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/coep-dom-chrome155.json"
    ))
    .unwrap();
    let mut seed = CookieJar::new();
    seed.set_from_response(&source_url, &["seed=local; HttpOnly; Path=/".into()], 0.0);
    for expected in control["result"].as_array().unwrap() {
        let policy = expected["policy"].as_str().unwrap();
        requests.lock().unwrap().clear();
        let headers = if policy == "unsafe-none" {
            Vec::new()
        } else {
            vec![("Cross-Origin-Embedder-Policy".into(), policy.into())]
        };
        let html = include_str!("../../../../harness/detection/fixtures/coep-dom.html")
            .replace("{{CROSS}}", &cross_url);
        let mut page = PageSession::open_with_response_headers(
            &html,
            &format!("{source_url}/page"),
            &seed.storage_state(),
            "",
            10000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/coep-dom-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page
            .eval(&format!("globalThis.__RESULT=JSON.stringify({probe})"))
            .await
            .unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, expected["rows"], "{policy}");
        let mut requests = requests.lock().unwrap().clone();
        requests.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        assert_eq!(
            json!(requests),
            expected["requests"],
            "actual DOM resource headers for {policy}"
        );
    }
    server.abort();
    drop(source);
}

#[tokio::test(flavor = "current_thread")]
async fn native_embedder_redirect_policy_and_cookie_hops_match_chrome() {
    let source = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cross = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source_url = format!("http://{}", source.local_addr().unwrap());
    let cross_url = format!("http://{}", cross.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut servers = Vec::new();
    for (listener, is_source) in [(source, true), (cross, false)] {
        let log = requests.clone();
        let source = source_url.clone();
        let cross = cross_url.clone();
        servers.push(tokio::spawn(async move {
            while let Ok((mut socket,_))=listener.accept().await {
                let log=log.clone();let source=source.clone();let cross=cross.clone();
                tokio::spawn(async move {
                    let mut bytes=Vec::new();loop {let mut buffer=[0;1024];let count=socket.read(&mut buffer).await.unwrap();if count==0{return;}bytes.extend_from_slice(&buffer[..count]);if bytes.windows(4).any(|row|row==b"\r\n\r\n"){break;}}
                    let request=String::from_utf8_lossy(&bytes);let path=request.split_whitespace().nth(1).unwrap();
                    let cookie=request.lines().filter_map(|line|line.split_once(':')).find(|(name,_)|name.eq_ignore_ascii_case("cookie")).map(|(_,value)|value.trim()).unwrap_or("");
                    log.lock().unwrap().push(json!({"side":if is_source{"source"}else{"cross"},"path":path,"cookie":cookie}));
                    let redirect=matches!(path,"/source-open"|"/source-authorized"|"/cross-back"|"/cross-open-back");
                    let corp=if matches!(path,"/source-authorized"|"/final-source-authorized"|"/cross-back"){"Cross-Origin-Resource-Policy: cross-origin\r\n"}else{""};
                    let location=if redirect{format!("Location: {}/final-{}\r\n",if is_source{cross}else{source},&path[1..])}else{String::new()};
                    let body="authored redirect policy resource";
                    socket.write_all(format!("HTTP/1.1 {} OK\r\nContent-Type: text/plain\r\n{corp}{location}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",if redirect{302}else{200},body.len()).as_bytes()).await.unwrap();
                });
            }
        }));
    }
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/coep-redirect-chrome155.json"
    ))
    .unwrap();
    let mut seed = CookieJar::new();
    seed.set_from_response(&source_url, &["seed=local; HttpOnly; Path=/".into()], 0.0);
    for expected in control["result"].as_array().unwrap() {
        let policy = expected["policy"].as_str().unwrap();
        requests.lock().unwrap().clear();
        let headers = if policy == "unsafe-none" {
            Vec::new()
        } else {
            vec![("Cross-Origin-Embedder-Policy".into(), policy.into())]
        };
        let settings = json!({"source":source_url,"cross":cross_url});
        let init = format!("globalThis.__coepRedirect={settings};");
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            &format!("{source_url}/page"),
            &seed.storage_state(),
            "",
            10000,
            &init,
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/coep-redirect-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page
            .eval(&format!(
                "(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})()"
            ))
            .await
            .unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, expected["rows"], "{policy}");
        assert_eq!(
            json!(*requests.lock().unwrap()),
            expected["requests"],
            "actual redirect hop headers for {policy}"
        );
    }
    for server in servers {
        server.abort();
    }
}
