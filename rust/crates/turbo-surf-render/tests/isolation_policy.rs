use serde_json::Value;
use turbo_surf_render::PageSession;

fn verify_control_sources(
    control: &Value,
    cases: &[u8],
    probe: &[u8],
    collector: &[u8],
    child: &[u8],
) {
    use sha2::Digest;
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(control["browser"], "Chrome/155.0.8059.79");
    assert_eq!(control["v8"], "15.5.35.23");
    for (key, bytes) in [
        ("casesSha256", cases),
        ("probeSha256", probe),
        ("collectorSha256", collector),
        ("childProbeSha256", child),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", sha2::Sha256::digest(bytes)),
            "stale control: {key}"
        );
    }
    let values: Value = serde_json::from_slice(cases).unwrap();
    assert_eq!(
        values.as_array().unwrap().len(),
        control["result"].as_array().unwrap().len()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unisolated_shared_memory_matches_authored_chrome() {
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body></body>",
        "http://localhost/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/shared-memory-unisolated-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/shared-memory-unisolated-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn permissions_policy_brand_descriptors_and_defaults_match_chrome() {
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body></body>",
        "http://localhost/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/permissions-shape-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-shape-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn permissions_policy_header_parsing_matches_chrome() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-policy-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-policy-chrome155.json"
    ))
    .unwrap();
    for (case, control) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            "http://localhost/",
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/permissions-policy-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            actual, control["result"],
            "case {}: {}",
            case["id"], case["headers"]
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn isolated_header_capability_matches_chrome() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/isolation-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/isolation-chrome155.json"
    ))
    .unwrap();
    for (case, control) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            "http://localhost/",
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/isolation-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            actual, control["result"],
            "case {}: {}",
            case["id"], case["headers"]
        );
    }
}

async fn frame_server(
    cases: Value,
    child_source: &'static str,
) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let cases = std::sync::Arc::new(cases);
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let cases = cases.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let size = socket.read(&mut chunk).await.unwrap();
                    if size == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..size]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap();
                let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
                let index = url
                    .query_pairs()
                    .find(|(name, _)| name == "case")
                    .unwrap()
                    .1
                    .parse::<usize>()
                    .unwrap();
                let fields = if url
                    .query_pairs()
                    .any(|(name, value)| name == "depth" && value == "2")
                    && cases[index]["leafHeaders"].is_array()
                {
                    &cases[index]["leafHeaders"]
                } else {
                    &cases[index]["childHeaders"]
                };
                let headers: Vec<(String, String)> =
                    serde_json::from_value(fields.clone()).unwrap();
                let headers = headers
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}\r\n"))
                    .collect::<String>();
                let cookie = request
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                            .map(|(_, value)| value.trim())
                    })
                    .unwrap_or("");
                let echo = cases[index]["captureCookies"] == true
                    && url.query_pairs().any(|(name, _)| name == "echo");
                let (mime, body) = if echo {
                    (
                        "application/json",
                        serde_json::json!({"cookie":cookie}).to_string(),
                    )
                } else {
                    let prefix = if cases[index]["captureCookies"] == true {
                        format!("globalThis.__requestCookie={};", serde_json::json!(cookie))
                    } else {
                        String::new()
                    };
                    (
                        "text/html",
                        format!("<!doctype html><script>{prefix}{child_source}</script>"),
                    )
                };
                let headers = if echo { String::new() } else { headers };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    (origin, task)
}

#[tokio::test(flavor = "current_thread")]
async fn iframe_embedding_and_permissions_policy_match_chrome() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-frame-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-frame-chrome155.json"
    ))
    .unwrap();
    let (origin, server) = frame_server(
        cases.clone(),
        include_str!("../../../../harness/detection/policy-frame-child.js"),
    )
    .await;
    for (index, (case, control)) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
        .enumerate()
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let html = format!(
            "<!doctype html><script>globalThis.__policyFrameCase={case}</script><body></body>"
        );
        let mut page = PageSession::open_with_response_headers(
            &html,
            &format!("{origin}/?case={index}"),
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/policy-frame-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}: {case}", case["id"]);
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn shared_memory_window_delivery_matches_chrome_agent_clusters() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-shared-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-shared-chrome155.json"
    ))
    .unwrap();
    let (origin, server) = frame_server(
        cases.clone(),
        include_str!("../../../../harness/detection/policy-shared-child.js"),
    )
    .await;
    for (index, (case, control)) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
        .enumerate()
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let html = format!(
            "<!doctype html><script>globalThis.__policyFrameCase={case}</script><body></body>"
        );
        let mut page = PageSession::open_with_response_headers(
            &html,
            &format!("{origin}/?case={index}"),
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/policy-shared-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}: {case}", case["id"]);
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn credentialless_cookie_and_dom_storage_partitions_match_chrome() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-credentialless-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-credentialless-chrome155.json"
    ))
    .unwrap();
    let (origin, server) = frame_server(
        cases.clone(),
        include_str!("../../../../harness/detection/policy-credentialless-child.js"),
    )
    .await;
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        &format!("{origin}/?case=0"),
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/policy-credentialless-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(actual, controls["result"][0]["result"]);
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn denied_permissions_query_state_matches_chrome() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-effects-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-effects-chrome155.json"
    ))
    .unwrap();
    for (case, control) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            "http://localhost/",
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/permissions-effects-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}", case["id"]);
        if case["id"] == "0" {
            let raw = page.eval_json_expression("(async()=>{const errors=[];for(const operation of [()=>navigator.clipboard.readText(),()=>navigator.clipboard.writeText('value')])try{await operation();errors.push('accepted');}catch(error){errors.push(error.name);}return errors;})()").await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&raw).unwrap(),
                serde_json::json!(["NotAllowedError", "NotAllowedError"])
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn shared_memory_queue_accounting_preserves_admission_and_growth() {
    let headers = vec![
        ("Cross-Origin-Opener-Policy".into(), "same-origin".into()),
        ("Cross-Origin-Embedder-Policy".into(), "require-corp".into()),
    ];
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body></body>",
        "http://localhost/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    page.set_message_port_limits_for_testing(8, 65536);
    let raw=page.eval_json_expression("(()=>{const channel=new MessageChannel(),buffer=new ArrayBuffer(4),shared=new SharedArrayBuffer(65536);let error=null;try{channel.port1.postMessage({shared,buffer},[buffer]);}catch(caught){error=caught.name;}channel.port1.close();channel.port2.close();return {error,bytes:buffer.byteLength};})()").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        serde_json::json!({"error":"RangeError","bytes":4})
    );
    assert_eq!(
        page.message_port_diagnostics_for_testing()["queuedBytes"],
        0
    );
    page.set_message_port_limits_for_testing(8, 1048576);
    page.eval_expression("(()=>{globalThis.memory=new WebAssembly.Memory({initial:1,maximum:2,shared:true});globalThis.channel=new MessageChannel();channel.port2.onmessage=()=>{};channel.port1.postMessage(memory.buffer);memory.grow(1);return 'queued';})()").await.unwrap();
    let queued = page.message_port_diagnostics_for_testing()["queuedBytes"]
        .as_u64()
        .unwrap();
    assert!(
        (65536..65600).contains(&queued),
        "unexpected shared snapshot accounting: {queued}"
    );
    page.eval("globalThis.__RESULT='';").await.unwrap();
    assert_eq!(
        page.message_port_diagnostics_for_testing()["queuedBytes"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn permissions_wildcard_origins_and_enumeration_order_match_chrome() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-origins-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-origins-chrome155.json"
    ))
    .unwrap();
    for (case, control) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            "http://127.0.0.1/",
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/permissions-origins-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            actual, control["result"],
            "case {}: {}",
            case["id"], case["headers"]
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn permission_status_real_brands_prototypes_and_events_match_chrome() {
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "http://localhost/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/permission-status-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permission-status-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn iframe_container_policy_mutates_without_rewriting_committed_child_policy() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-dynamic-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-dynamic-chrome155.json"
    ))
    .unwrap();
    let (origin, server) = frame_server(
        cases.clone(),
        include_str!("../../../../harness/detection/policy-frame-child.js"),
    )
    .await;
    for (index, (case, control)) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
        .enumerate()
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            &format!("{origin}/?case={index}"),
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/permissions-dynamic-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}", case["id"]);
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn deep_permissions_admission_cannot_reenable_denied_ancestors() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-deep-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-deep-chrome155.json"
    ))
    .unwrap();
    let (origin, server) = frame_server(
        cases.clone(),
        include_str!("../../../../harness/detection/policy-deep-child.js"),
    )
    .await;
    for (index, (case, control)) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
        .enumerate()
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let html = format!(
            "<!doctype html><script>globalThis.__policyFrameCase={case}</script><body></body>"
        );
        let mut page = PageSession::open_with_response_headers(
            &html,
            &format!("{origin}/?case={index}"),
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/policy-deep-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}: {case}", case["id"]);
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn permissions_query_dictionary_errors_and_actual_interface_match_chrome() {
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "http://localhost/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/permissions-query-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/permissions-query-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn response_ancestor_and_legacy_frame_policies_match_chrome() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-ancestors-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-ancestors-chrome155.json"
    ))
    .unwrap();
    verify_control_sources(
        &controls,
        include_bytes!("../../../../harness/detection/fixtures/policy-ancestors-cases.json"),
        include_bytes!("../../../../harness/detection/policy-frame-probe.js"),
        include_bytes!("../../../../harness/detection/policy-frame-chrome.mjs"),
        include_bytes!("../../../../harness/detection/policy-frame-child.js"),
    );
    let (origin, server) = frame_server(
        cases.clone(),
        include_str!("../../../../harness/detection/policy-frame-child.js"),
    )
    .await;
    for (index, (case, control)) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
        .enumerate()
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let html = format!(
            "<!doctype html><script>globalThis.__policyFrameCase={case}</script><body></body>"
        );
        let mut page = PageSession::open_with_response_headers(
            &html,
            &format!("{origin}/?case={index}"),
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/policy-frame-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}: {case}", case["id"]);
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn ancestor_response_policies_check_each_actual_parent_origin() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-ancestor-tree-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-ancestor-tree-chrome155.json"
    ))
    .unwrap();
    verify_control_sources(
        &controls,
        include_bytes!("../../../../harness/detection/fixtures/policy-ancestor-tree-cases.json"),
        include_bytes!("../../../../harness/detection/policy-deep-probe.js"),
        include_bytes!("../../../../harness/detection/policy-ancestors-chrome.mjs"),
        include_bytes!("../../../../harness/detection/policy-ancestor-tree-child.js"),
    );
    let (origin, server) = frame_server(
        cases.clone(),
        include_str!("../../../../harness/detection/policy-ancestor-tree-child.js"),
    )
    .await;
    for (index, (case, control)) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
        .enumerate()
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let html = format!(
            "<!doctype html><script>globalThis.__policyFrameCase={case}</script><body></body>"
        );
        let mut page = PageSession::open_with_response_headers(
            &html,
            &format!("{origin}/?case={index}"),
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/policy-deep-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}: {case}", case["id"]);
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn response_sandbox_script_origin_and_storage_flags_match_chrome() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-sandbox-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-sandbox-chrome155.json"
    ))
    .unwrap();
    verify_control_sources(
        &controls,
        include_bytes!("../../../../harness/detection/fixtures/policy-sandbox-cases.json"),
        include_bytes!("../../../../harness/detection/policy-sandbox-probe.js"),
        include_bytes!("../../../../harness/detection/policy-frame-chrome.mjs"),
        include_bytes!("../../../../harness/detection/policy-sandbox-child.js"),
    );
    let (origin, server) = frame_server(
        cases.clone(),
        include_str!("../../../../harness/detection/policy-sandbox-child.js"),
    )
    .await;
    for (index, (case, control)) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
        .enumerate()
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let html = format!(
            "<!doctype html><script>globalThis.__policyFrameCase={case}</script><body></body>"
        );
        let mut page = PageSession::open_with_response_headers(
            &html,
            &format!("{origin}/?case={index}"),
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/policy-sandbox-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}: {case}", case["id"]);
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn top_response_sandbox_disables_guest_scripts_and_opaque_storage() {
    use sha2::Digest;
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-sandbox-top-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-sandbox-top-chrome155.json"
    ))
    .unwrap();
    assert_eq!(controls["errors"], serde_json::json!([]));
    assert_eq!(
        controls["casesSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/fixtures/policy-sandbox-top-cases.json"
            ))
        )
    );
    assert_eq!(
        controls["probeSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/policy-sandbox-top-probe.js"
            ))
        )
    );
    assert_eq!(
        controls["collectorSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/policy-header-chrome.mjs"
            ))
        )
    );
    assert_eq!(
        cases.as_array().unwrap().len(),
        controls["result"].as_array().unwrap().len()
    );
    for (case, control) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            "http://127.0.0.1:12345/",
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/policy-sandbox-top-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}: {case}", case["id"]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn top_sandbox_module_requests_use_authentic_opaque_origin_and_cookie_site() {
    use sha2::Digest;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-sandbox-module-cases.json"
    ))
    .unwrap();
    let controls: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/policy-sandbox-module-chrome155.json"
    ))
    .unwrap();
    assert_eq!(controls["errors"], serde_json::json!([]));
    assert_eq!(
        controls["casesSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/fixtures/policy-sandbox-module-cases.json"
            ))
        )
    );
    assert_eq!(
        controls["probeSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/policy-sandbox-module-probe.js"
            ))
        )
    );
    assert_eq!(
        controls["collectorSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/policy-sandbox-module-chrome.mjs"
            ))
        )
    );
    assert_eq!(
        cases.as_array().unwrap().len(),
        controls["result"].as_array().unwrap().len()
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let count = socket.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    if bytes.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let field = |name: &str| {
                    request
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                                .map(|(_, value)| value.trim())
                        })
                        .unwrap_or("")
                };
                let body = format!(
                    "export const observed={};",
                    serde_json::json!({"origin":field("origin"),"cookie":field("cookie")})
                );
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nAccess-Control-Allow-Origin: null\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    for (index, (case, control)) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(controls["result"].as_array().unwrap())
        .enumerate()
    {
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let mut jar = turbo_surf_core::cookies::CookieJar::new();
        jar.set_from_response(
            &origin,
            &["sandboxTransport=1;Path=/;SameSite=Lax".to_string()],
            0.0,
        );
        let cookies = jar.storage_state();
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body></body>",
            &format!("{origin}/?case={index}"),
            &cookies,
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/policy-sandbox-module-probe.js")
            .trim()
            .trim_end_matches(';');
        let raw = page.eval_json_expression(probe).await.unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(actual, control["result"], "case {}: {case}", case["id"]);
    }
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn security_policy_violation_event_original_fields_and_brands_match_chrome() {
    use sha2::Digest;
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/security-policy-event-chrome155.json"
    ))
    .unwrap();
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["collectorSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/authored-dom-chrome.mjs"
            ))
        )
    );
    assert_eq!(
        control["probeSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/security-policy-event-probe.js"
            ))
        )
    );
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "http://localhost/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/security-policy-event-probe.js")
        .trim()
        .trim_end_matches(';');
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(actual, control["result"]);
}

fn csp_violation_control() -> Value {
    use sha2::Digest;
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/csp-violation-chrome155.json"
    ))
    .unwrap();
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(control["browser"], "Chrome/155.0.8059.79");
    assert_eq!(control["v8"], "15.5.35.23");
    for (key, bytes) in [
        (
            "casesSha256",
            include_bytes!("../../../../harness/detection/fixtures/csp-violation-cases.json")
                .as_slice(),
        ),
        (
            "probeSha256",
            include_bytes!("../../../../harness/detection/csp-violation-probe.js").as_slice(),
        ),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/policy-header-chrome.mjs").as_slice(),
        ),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", sha2::Sha256::digest(bytes)),
            "stale control: {key}"
        );
    }
    assert_eq!(control["result"].as_array().unwrap().len(), 2);
    control
}

#[tokio::test(flavor = "current_thread")]
async fn enforced_csp_violation_tasks_match_chrome() {
    let headers = vec![(
        "Content-Security-Policy".to_string(),
        "script-src 'none'; style-src 'none'; connect-src 'none'".to_string(),
    )];
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body></body>",
        "http://127.0.0.1:12345/?case=0",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/csp-violation-probe.js");
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let control = csp_violation_control();
    assert_eq!(actual, control["result"][0]["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn report_only_csp_violation_tasks_match_chrome() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut bytes = [0; 4096];
                if socket.read(&mut bytes).await.unwrap() == 0 {
                    return;
                }
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            });
        }
    });
    let headers = vec![(
        "Content-Security-Policy-Report-Only".to_string(),
        "script-src 'none'; style-src 'none'; connect-src 'none'".to_string(),
    )];
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body></body>",
        &format!("{origin}/?case=1"),
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/csp-violation-probe.js");
    let raw = page.eval_json_expression(probe).await.unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let control = csp_violation_control();
    assert_eq!(actual, control["result"][1]["result"]);
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn csp_task_capacity_is_bounded_and_recovers_after_dispatch() {
    let headers = vec![("Content-Security-Policy".into(), "style-src 'none'".into())];
    let mut page = PageSession::open_with_response_headers(
        "<body></body>",
        "https://budget.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let error = page
        .eval_expression(
            r#"(() => {
        globalThis.attempts=0;globalThis.violations=0;
        document.addEventListener('securitypolicyviolation',()=>violations++);
        const div=document.createElement('div');document.body.append(div);
        for(let index=0;index<5000;index++){
          attempts++;div.setAttribute('style','color:rgb('+index+',0,0)');
        }
        return attempts;
    })()"#,
        )
        .await
        .unwrap_err();
    assert!(
        error.contains("terminat") || error.contains("execution"),
        "{error}"
    );
    let result: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                r#"(async()=>{
        await new Promise(resolve=>setTimeout(resolve,30));
        const first=violations;
        document.querySelector('div').setAttribute('style','color:blue');
        await new Promise(resolve=>setTimeout(resolve,30));
        return {attempts,first,after:violations};
    })()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        result,
        serde_json::json!({"attempts":4097,"first":4096,"after":4097})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn inline_classic_and_module_execution_do_not_dispatch_load_events() {
    use sha2::Digest;
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/module-inline-load-chrome155.json"
    ))
    .unwrap();
    let probe = include_str!("../../../../harness/detection/module-inline-load-probe.js");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["collectorSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(include_bytes!(
                "../../../../harness/detection/authored-dom-chrome.mjs"
            ))
        )
    );
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", sha2::Sha256::digest(probe.as_bytes()))
    );
    let mut page = PageSession::open("<body></body>", "https://module.test/", "", "", 5000)
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn csp_event_tasks_release_before_reentrant_guest_dispatch_and_survive_overrides() {
    let headers = vec![("Content-Security-Policy".into(), "style-src 'none'".into())];
    let mut page = PageSession::open_with_response_headers(
        "<body><div></div></body>",
        "https://events.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                r#"(async()=>{
      const timer=setTimeout, originalCtor=SecurityPolicyViolationEvent;
      const div=document.querySelector('div'),rows=[];
      globalThis.SecurityPolicyViolationEvent=()=>{throw Error('guest constructor');};
      globalThis.setTimeout=()=>{throw Error('guest timer');};
      globalThis.clearTimeout=()=>{throw Error('guest cancellation');};
      document.addEventListener('securitypolicyviolation',event=>{
        rows.push([event instanceof originalCtor,event.isTrusted,event.target===div]);
        if(rows.length===1){div.setAttribute('style','color:blue');throw Error('listener failure');}
      });
      div.setAttribute('style','color:red');
      await new Promise(resolve=>timer(resolve,50));
      return rows;
    })()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        actual,
        serde_json::json!([[true, true, true], [true, true, true]])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retiring_frames_cancels_policy_event_tasks_without_retaining_realms() {
    let headers = vec![("Content-Security-Policy".into(), "style-src 'none'".into())];
    let mut page = PageSession::open_with_response_headers(
        "<body><iframe id=f></iframe></body>",
        "https://events.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let actual:Value=serde_json::from_str(&page.eval_json_expression(r#"(async()=>{
       globalThis.retiredPolicyEvents=0;
       const frame=document.getElementById('f'),child=frame.contentWindow;
       child.eval("document.addEventListener('securitypolicyviolation',()=>parent.retiredPolicyEvents++);const div=document.createElement('div');document.body.append(div);div.setAttribute('style','color:red');");
       frame.remove();
       await new Promise(resolve=>setTimeout(resolve,50));
       return retiredPolicyEvents;
    })()"#).await.unwrap()).unwrap();
    assert_eq!(actual, serde_json::json!(0));
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["retiredHostContexts"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn parser_policy_events_survive_bootstrap_until_authored_listeners_execute() {
    use sha2::Digest;
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/csp-parser-cases.json"
    ))
    .unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/csp-parser-chrome155.json"
    ))
    .unwrap();
    for (key, bytes) in [
        (
            "casesSha256",
            include_bytes!("../../../../harness/detection/fixtures/csp-parser-cases.json")
                .as_slice(),
        ),
        (
            "probeSha256",
            include_bytes!("../../../../harness/detection/csp-parser-probe.js").as_slice(),
        ),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/csp-document-chrome.mjs").as_slice(),
        ),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", sha2::Sha256::digest(bytes)),
            "stale control: {key}"
        );
    }
    assert_eq!(control["errors"], serde_json::json!([]));
    let headers = cases[0]["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row[0].as_str().unwrap().to_string(),
                row[1].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let mut page = PageSession::open_with_response_headers(
        cases[0]["html"].as_str().unwrap(),
        "http://127.0.0.1:12345/?case=0",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/csp-parser-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    assert_eq!(actual, control["result"][0]["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn original_parser_source_positions_and_report_order_match_fresh_chrome() {
    use sha2::Digest;
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/parser-source-position-cases.json"
    ))
    .unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/parser-source-position-chrome155.json"
    ))
    .unwrap();
    for (key, bytes) in [
        (
            "casesSha256",
            include_bytes!(
                "../../../../harness/detection/fixtures/parser-source-position-cases.json"
            )
            .as_slice(),
        ),
        (
            "probeSha256",
            include_bytes!("../../../../harness/detection/parser-source-position-probe.js")
                .as_slice(),
        ),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/csp-document-chrome.mjs").as_slice(),
        ),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", sha2::Sha256::digest(bytes)),
            "stale control: {key}"
        );
    }
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(control["browser"], "Chrome/155.0.8059.79");
    assert_eq!(control["v8"], "15.5.35.23");
    assert_eq!(cases.as_array().unwrap().len(), 14);
    assert_eq!(control["result"].as_array().unwrap().len(), 14);
    for (case, oracle) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(control["result"].as_array().unwrap())
    {
        let headers = case["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row[0].as_str().unwrap().to_string(),
                    row[1].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>();
        let base = format!(
            "http://127.0.0.1:12345/?case={}",
            case["id"].as_str().unwrap()
        );
        let mut page = PageSession::open_with_response_headers(
            case["html"].as_str().unwrap(),
            &base,
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/parser-source-position-probe.js")
            .trim()
            .trim_end_matches(';');
        let actual: Value =
            serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
        assert_eq!(actual, oracle["result"], "case {}", case["id"]);
    }
}

async fn resource_csp_results() -> Value {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/csp-resource-cases.json"
    ))
    .unwrap();
    let headers = cases[0]["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row[0].as_str().unwrap().to_string(),
                row[1].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let mut page = PageSession::open_with_response_headers(
        "<body>",
        "http://127.0.0.1:12345/?case=0",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/csp-resource-probe.js")
        .trim()
        .trim_end_matches(';');
    serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap()
}
fn resource_csp_control() -> Value {
    use sha2::Digest;
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/csp-resource-chrome155.json"
    ))
    .unwrap();
    for (key, bytes) in [
        (
            "casesSha256",
            include_bytes!("../../../../harness/detection/fixtures/csp-resource-cases.json")
                .as_slice(),
        ),
        (
            "probeSha256",
            include_bytes!("../../../../harness/detection/csp-resource-probe.js").as_slice(),
        ),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/csp-resource-chrome.mjs").as_slice(),
        ),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", sha2::Sha256::digest(bytes)),
            "stale control {key}"
        );
    }
    assert!(
        include_str!("../../../../harness/detection/csp-resource-chrome.mjs")
            .contains("allowUnsafeEvalBlockedByCSP: false")
    );
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["result"][0]["result"]["rows"]
            .as_array()
            .unwrap()
            .len(),
        19
    );
    control["result"][0]["result"].clone()
}
#[tokio::test(flavor = "current_thread")]
async fn external_resource_reports_and_original_utf16_samples_match_chrome() {
    let actual = resource_csp_results().await;
    let expected = resource_csp_control();
    #[cfg(not(turbo_surf_native_proxy))]
    let expected = {
        // The default SDK lacks the native callback; preserve the complete
        // source-pinned oracle and compare its covered resource/sample rows.
        let mut expected = expected;
        let rows = expected["rows"].as_array_mut().unwrap();
        assert_eq!(
            rows.iter().filter(|row| row["blocked"] == "eval").count(),
            2
        );
        rows.retain(|row| row["blocked"] != "eval");
        expected
    };
    assert_eq!(actual, expected);
}
#[tokio::test(flavor = "current_thread")]
#[cfg_attr(
    not(turbo_surf_native_proxy),
    ignore = "requires actual native V8 code-generation callback SDK"
)]
async fn native_eval_and_resource_csp_reports_match_full_chrome_control() {
    assert_eq!(resource_csp_results().await, resource_csp_control());
}

#[tokio::test(flavor = "current_thread")]
#[cfg_attr(
    not(turbo_surf_native_proxy),
    ignore = "requires actual native V8 code-generation callback SDK"
)]
async fn native_code_generation_preserves_lexical_identity_and_report_only_controls() {
    use sha2::Digest;
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/csp-code-generation-cases.json"
    ))
    .unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/csp-code-generation-chrome155.json"
    ))
    .unwrap();
    for (key, bytes) in [
        (
            "casesSha256",
            include_bytes!("../../../../harness/detection/fixtures/csp-code-generation-cases.json")
                .as_slice(),
        ),
        (
            "probeSha256",
            include_bytes!("../../../../harness/detection/csp-code-generation-probe.js").as_slice(),
        ),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/csp-resource-chrome.mjs").as_slice(),
        ),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", sha2::Sha256::digest(bytes)),
            "stalecontrol {key}"
        );
    }
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(cases.as_array().unwrap().len(), 7);
    assert_eq!(control["result"].as_array().unwrap().len(), 7);
    for (case, oracle) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(control["result"].as_array().unwrap())
    {
        let headers = case["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row[0].as_str().unwrap().to_string(),
                    row[1].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>();
        let base = format!(
            "http://127.0.0.1:12345/?case={}",
            case["id"].as_str().unwrap()
        );
        let mut page = PageSession::open_with_response_headers(
            "<body>",
            &base,
            "",
            "",
            5000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let probe = include_str!("../../../../harness/detection/csp-code-generation-probe.js")
            .trim()
            .trim_end_matches(';');
        let actual: Value =
            serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
        assert_eq!(actual, oracle["result"], "case {}", case["id"]);
    }
}

#[tokio::test(flavor = "current_thread")]
#[cfg_attr(
    not(turbo_surf_native_proxy),
    ignore = "requires actual native V8 code-generation callback SDK"
)]
async fn native_code_generation_tasks_bound_capacity_and_release_before_reentry() {
    let headers = vec![(
        "Content-Security-Policy".into(),
        "script-src 'none' 'report-sample'".into(),
    )];
    let mut page = PageSession::open_with_response_headers(
        "<body>",
        "https://eval-budget.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let error = page
        .eval_expression(
            r#"(()=>{
      globalThis.attempts=0;globalThis.violations=0;
      document.addEventListener('securitypolicyviolation',()=>violations++);
      for(let index=0;index<5000;index++){attempts++;try{eval('/*'+index+'*/')}catch{}}
      return attempts;
    })()"#,
        )
        .await
        .unwrap_err();
    assert!(
        error.contains("terminat") || error.contains("execution"),
        "{error}"
    );
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                r#"(async()=>{
      await new Promise(resolve=>setTimeout(resolve,30));
      const first=violations;
      let reentered=false;
      document.addEventListener('securitypolicyviolation',()=>{
        if(!reentered){reentered=true;try{eval('/*reentrant*/')}catch{}}
      });
      try{eval('/*after dispatch*/')}catch{}
      await new Promise(resolve=>setTimeout(resolve,30));
      return {attempts,first,after:violations,reentered};
    })()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        actual,
        serde_json::json!({"attempts":4097,"first":4096,"after":4098,"reentered":true})
    );
}
