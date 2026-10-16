use base64::Engine;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn response_csp_enforces_nonce_hash_module_graph_and_blocked_network_resources() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let observed = observed.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                observed.lock().unwrap().push(path.to_string());
                let body = match path {
                    "/allowed.js" => "globalThis.allowedClassic=true;",
                    "/allowed.mjs" => {
                        "import {allowed} from './dep.mjs';globalThis.allowedModule=allowed;"
                    }
                    "/dep.mjs" => "export const allowed=true;",
                    "/blocked.js" => "globalThis.blockedExternal=true;",
                    _ => "",
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let hash = base64::engine::general_purpose::STANDARD
        .encode(Sha256::digest("globalThis.hashAllowed=true;"));
    let headers=vec![("Content-Security-Policy".into(),format!("script-src 'nonce-good' 'sha256-{hash}'; connect-src 'none'; style-src 'none'; img-src 'none'; frame-src 'none'"))];
    let mut page = PageSession::open_with_response_headers(
        include_str!("../../../../harness/detection/fixtures/csp.html"),
        &format!("http://{address}/page"),
        "",
        "",
        10000,
        "",
        ClientHints::default(),
        &headers,
    )
    .await
    .unwrap();
    assert!(
        !page.hydration_diagnostics().budget_exceeded,
        "{:?}",
        page.hydration_diagnostics()
    );
    let result = page
        .eval("globalThis.__RESULT=JSON.stringify(__browserFixture)")
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    for key in [
        "inlineBlocked",
        "hashAllowed",
        "nonceAllowed",
        "evalBlocked",
        "connectBlocked",
        "classic",
        "module",
        "externalBlocked",
    ] {
        assert_eq!(value[key], true, "{key}: {value}");
    }
    let requests = requests.lock().unwrap();
    for path in ["/allowed.js", "/allowed.mjs", "/dep.mjs"] {
        assert!(requests.iter().any(|item| item == path), "{requests:?}");
    }
    for path in [
        "/blocked.js",
        "/blocked-connect",
        "/blocked.css",
        "/blocked.png",
        "/blocked-frame",
    ] {
        assert!(!requests.iter().any(|item| item == path), "{requests:?}");
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn meta_policy_applies_at_parser_position_and_repeated_headers_intersect() {
    let html="<head><script>globalThis.beforeMeta=true;</script><meta http-equiv='Content-Security-Policy' content=\"script-src 'none'\"><script>globalThis.afterMeta=true;</script></head><body></body>";
    let mut page = PageSession::open(html, "http://policy.test/page", "", "", 10000)
        .await
        .unwrap();
    let result=page.eval("globalThis.__RESULT=JSON.stringify({before:globalThis.beforeMeta===true,after:globalThis.afterMeta===undefined})").await.unwrap();
    assert_eq!(result, "{\"before\":true,\"after\":true}");
    page.close();
    let headers = vec![
        (
            "content-security-policy".into(),
            "script-src 'unsafe-inline'".into(),
        ),
        (
            "content-security-policy".into(),
            "default-src 'none'".into(),
        ),
    ];
    let mut page = PageSession::open_with_response_headers(
        "<script>globalThis.executed=true;</script>",
        "http://policy.test/page",
        "",
        "",
        10000,
        "",
        ClientHints::default(),
        &headers,
    )
    .await
    .unwrap();
    let result = page
        .eval("globalThis.__RESULT=String(globalThis.executed===undefined)")
        .await
        .unwrap();
    assert_eq!(result, "true");
}

#[tokio::test(flavor = "current_thread")]
async fn hydration_diagnostics_are_native_and_only_report_real_budget_termination() {
    let mut page = PageSession::open(
        "<body id='partial'><script>while(true){}</script></body>",
        "http://policy.test/",
        "",
        "",
        50,
    )
    .await
    .unwrap();
    assert!(page.hydration_diagnostics().budget_exceeded);
    assert!(page.hydration_diagnostics().open_error.is_some());
    assert!(page.serialize().contains("id=\"partial\""));
    page.eval("globalThis.hydration_diagnostics={budget_exceeded:false};")
        .await
        .unwrap();
    assert!(page.hydration_diagnostics().budget_exceeded);
    page.close();
    let result = PageSession::open_with_init_script(
        "<body></body>",
        "http://policy.test/",
        "",
        "",
        1000,
        "throw new Error('execution application error')",
    )
    .await;
    assert!(matches!(result,Err(error)if error.contains("execution application error")));
}

#[tokio::test(flavor = "current_thread")]
async fn unsupported_active_csp_is_reported_from_native_policy_state() {
    let headers = vec![
        (
            "Content-Security-Policy".into(),
            "script-src 'nonce-good' 'strict-dynamic'; frame-ancestors 'none'; form-action 'self'"
                .into(),
        ),
        (
            "Content-Security-Policy-Report-Only".into(),
            "connect-src 'none'".into(),
        ),
    ];
    let mut page=PageSession::open_with_response_headers(r#"<head><meta http-equiv='Content-Security-Policy' content="worker-src 'none'"></head><body><script nonce=good>globalThis.unsupported_csp=[];</script></body>"#,"http://policy.test/page","","",10000,"",ClientHints::default(),&headers).await.unwrap();
    let expected = vec!["form-action", "script-src: 'strict-dynamic'", "worker-src"];
    assert_eq!(page.hydration_diagnostics().unsupported_csp, expected);
    page.eval("globalThis.hydration_diagnostics={unsupported_csp:[]};")
        .await
        .unwrap();
    assert_eq!(page.hydration_diagnostics().unsupported_csp, expected);
}

#[tokio::test(flavor = "current_thread")]
async fn inline_handler_csp_distinguishes_attributes_from_assigned_functions() {
    let headers = vec![(
        "Content-Security-Policy".into(),
        "script-src 'nonce-good'; script-src-attr 'none'".into(),
    )];
    let mut page=PageSession::open_with_response_headers(r#"<body><button id='blocked' onclick="globalThis.blockedHandler=true"></button><script nonce=good>globalThis.handlerWasNull=document.getElementById('blocked').onclick===null;document.getElementById('blocked').click();document.getElementById('blocked').onclick=()=>{globalThis.assignedHandler=true};document.getElementById('blocked').click();</script></body>"#,"http://policy.test/page","","",10000,"",ClientHints::default(),&headers).await.unwrap();
    let result=page.eval("globalThis.__RESULT=JSON.stringify([handlerWasNull,globalThis.blockedHandler===undefined,assignedHandler]);").await.unwrap();
    assert_eq!(result, "[true,true,true]");
    assert!(page.hydration_diagnostics().unsupported_csp.is_empty());
}
