//! CSS policy uses native document state while keeping authored text observable.
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn css_nonce_attribute_and_cssom_mutations_keep_chrome_effective_style_state() {
    let headers = vec![(
        "Content-Security-Policy".into(),
        "script-src 'nonce-good'; style-src-elem 'nonce-good'; style-src-attr 'none'".into(),
    )];
    let mut page = PageSession::open_with_response_headers(
        include_str!("../../../../harness/detection/fixtures/css-security.html"),
        "http://127.0.0.1:1/css-policy",
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
        "allowedSheet",
        "blockedSheet",
        "sheetApplies",
        "initialBlocked",
        "removed",
        "dynamicBlocked",
        "dynamicNonce",
        "privateHooks",
    ] {
        assert_eq!(value[key], true, "{key}: {value}");
    }
    assert_eq!(value["authored"], "opacity:0.25");
    assert_eq!(value["initialDeclaration"], "");
    for key in ["cssom", "deniedAttribute", "deniedAttrNode", "cssText"] {
        assert_eq!(value[key]["applies"], true, "{key}: {value}");
    }
    assert_eq!(value["deniedAttribute"]["attribute"], "opacity:0.75");
    assert_eq!(value["deniedAttrNode"]["attribute"], "opacity:0.8");
    assert_eq!(value["deniedAttribute"]["declaration"], "0.5");
    assert_eq!(value["deniedAttrNode"]["declaration"], "0.5");
}

#[tokio::test(flavor = "current_thread")]
async fn meta_css_policy_preserves_prior_style_authorization_and_checks_same_text_mutation() {
    let html = r#"<!doctype html><html><head><style id=early>#before {opacity:0.2}</style><meta http-equiv=Content-Security-Policy content="style-src 'none'"><style id=late>#after {opacity:0.4}</style><script nonce=good>window.addEventListener('load',()=>{const early=document.getElementById('early'),late=document.getElementById('late');const out={prior:!!early.sheet,later:late.sheet===null,priorApplies:getComputedStyle(document.getElementById('before')).opacity==='0.2',laterBlocked:getComputedStyle(document.getElementById('after')).opacity!=='0.4'};early.textContent=early.textContent;out.sameTextMutationBlocked=early.sheet===null;globalThis.__browserFixture=out;});</script></head><body><div id=before></div><div id=after></div></body></html>"#;
    let headers = vec![(
        "Content-Security-Policy".into(),
        "script-src 'nonce-good'; style-src 'unsafe-inline'".into(),
    )];
    let mut page = PageSession::open_with_response_headers(
        html,
        "http://127.0.0.1:1/meta-css-policy",
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
        .eval("globalThis.__RESULT=JSON.stringify(__browserFixture)")
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    for key in [
        "prior",
        "later",
        "priorApplies",
        "laterBlocked",
        "sameTextMutationBlocked",
    ] {
        assert_eq!(value[key], true, "{key}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn parser_resources_preserve_earlier_style_blocking_and_later_listener_registration() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source = format!("http://{}/page", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let count = stream.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let later = request.contains("GET /later.css ");
                if later {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                let body = if later {
                    "#later-target {opacity:0.4}"
                } else {
                    "#earlier-target {opacity:0.2}"
                };
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let html = r#"<!doctype html><html><head><link id=earlier rel=stylesheet href=/earlier.css onload="globalThis.earlierLoaded=true"><script>
    globalThis.stylesObserved=[];globalThis.earlierReady=!!document.getElementById('earlier').sheet&&globalThis.earlierLoaded===true;
    document.addEventListener('load',event=>{if(event.target.tagName==='LINK')stylesObserved.push([event.target.id,event.isTrusted]);},true);
    document.addEventListener('DOMContentLoaded',()=>{globalThis.laterAtDcl=!!document.getElementById('later').sheet;});
    window.addEventListener('load',()=>{globalThis.__browserFixture={earlierReady,laterAtDcl,observed:stylesObserved,laterReady:!!document.getElementById('later').sheet};});
    </script><link id=later rel=stylesheet href=/later.css></head><body><div id=earlier-target></div><div id=later-target></div></body></html>"#;
    let mut page = PageSession::open(html, &source, "", "", 60_000)
        .await
        .unwrap();
    let result = page
        .eval("globalThis.__RESULT=JSON.stringify(__browserFixture)")
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["earlierReady"], true, "{value}");
    assert_eq!(value["laterAtDcl"], false, "{value}");
    assert_eq!(value["laterReady"], true, "{value}");
    assert_eq!(
        value["observed"],
        serde_json::json!([["later", true]]),
        "{value}"
    );
    server.abort();
}
