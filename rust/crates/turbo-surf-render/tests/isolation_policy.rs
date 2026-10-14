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
