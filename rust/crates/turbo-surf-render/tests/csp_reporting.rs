use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::{BrowserStorage, DocumentResponseMetadata, PageSession};

const CASES: &str =
    include_str!("../../../../harness/detection/fixtures/csp-report-endpoint-cases.json");
const PROBE: &str = include_str!("../../../../harness/detection/csp-report-endpoint-probe.js");
const CERT: &[u8] = include_bytes!("csp_report_cert.der");
const KEY: &[u8] = include_bytes!("csp_report_key.der");
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[derive(Clone, Debug)]
struct Wire {
    path: String,
    method: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}
struct Server {
    port: u16,
    rows: Arc<Mutex<Vec<Wire>>>,
    closed: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn expand(text: &str, id: usize, port: u16, tls: bool) -> String {
    text.replace("{id}", &id.to_string())
        .replace("{port}", &port.to_string())
        .replace("http://", if tls { "https://" } else { "http://" })
}
fn headers(case: &Value, id: usize, port: u16, tls: bool) -> Vec<(String, String)> {
    let mut rows = vec![(
        "Referrer-Policy".into(),
        case["policy"]
            .as_str()
            .unwrap_or("strict-origin-when-cross-origin")
            .into(),
    )];
    for (key, name) in [
        ("csp", "Content-Security-Policy"),
        ("reportOnly", "Content-Security-Policy-Report-Only"),
        ("endpoints", "Reporting-Endpoints"),
    ] {
        if let Some(value) = case[key].as_str() {
            rows.push((name.into(), expand(value, id, port, tls)));
        }
    }
    if case["reportTo"].as_bool() == Some(true) {
        rows.push(("Report-To".into(),json!({"group":format!("g{id}"),"max_age":3600,"endpoints":[{"url":format!("https://127.0.0.1:{port}/report?case={id}")}]}).to_string()));
    }
    rows
}
fn html(case: &Value, id: usize, port: u16, tls: bool) -> String {
    let meta = case["meta"]
        .as_str()
        .map(|value| {
            format!(
                "<meta http-equiv='Content-Security-Policy' content=\"{}\">",
                expand(value, id, port, tls)
                    .replace('&', "&amp;")
                    .replace('"', "&quot;")
            )
        })
        .unwrap_or_default();
    format!("<!doctype html><head>{meta}</head><body></body>")
}
async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    port: u16,
    tls: bool,
    cases: Arc<Vec<Value>>,
    rows: Arc<Mutex<Vec<Wire>>>,
    closed: Arc<Mutex<Vec<String>>>,
) {
    let mut raw = Vec::new();
    let mut byte = [0u8; 1024];
    let end = loop {
        let Ok(n) = stream.read(&mut byte).await else {
            return;
        };
        if n == 0 {
            return;
        }
        raw.extend(&byte[..n]);
        if raw.len() > 1024 * 1024 {
            return;
        }
        if let Some(end) = raw.windows(4).position(|v| v == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let text = String::from_utf8_lossy(&raw[..end]);
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap().split_whitespace();
    let method = first.next().unwrap().to_string();
    let path = first.next().unwrap().to_string();
    let fields: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    let length = fields
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    while raw.len() < end + length {
        let Ok(n) = stream.read(&mut byte).await else {
            return;
        };
        if n == 0 {
            return;
        }
        raw.extend(&byte[..n]);
    }
    let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
    let id = url
        .query_pairs()
        .find(|(key, _)| key == "case")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let is_report = [
        "/report",
        "/fallback",
        "/hold-report",
        "/redirect302",
        "/redirect307",
        "/redirect-target",
    ]
    .contains(&url.path());
    if is_report {
        rows.lock().unwrap().push(Wire {
            path: path.clone(),
            method: method.clone(),
            headers: fields,
            body: raw[end..end + length].to_vec(),
        });
    }
    if url.path() == "/hold-report" {
        let _ = stream.read(&mut byte).await;
        closed.lock().unwrap().push(path);
        return;
    }
    let mut extra = String::new();
    let status;
    let body;
    if method == "OPTIONS" {
        status = 204;
        body = String::new();
        extra.push_str("Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST\r\nAccess-Control-Allow-Headers: content-type\r\n");
    } else if url.path() == "/redirect302" || url.path() == "/redirect307" {
        status = if url.path() == "/redirect302" {
            302
        } else {
            307
        };
        body = String::new();
        extra.push_str(&format!("Location: /redirect-target?case={id}\r\n"));
    } else if is_report {
        status = 204;
        body = String::new();
        extra.push_str(&format!("Access-Control-Allow-Origin: *\r\nSet-Cookie: report_response={id}; Path=/; SameSite=Lax\r\n"));
    } else {
        status = 200;
        body = html(&cases[id], id, port, tls);
        extra.push_str("Content-Type: text/html\r\n");
        for (name, value) in headers(&cases[id], id, port, tls) {
            extra.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    let response = format!(
        "HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}
impl Server {
    async fn start(tls: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let cases = Arc::new(serde_json::from_str::<Vec<Value>>(CASES).unwrap());
        let rows = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(Mutex::new(Vec::new()));
        let output = rows.clone();
        let close_output = closed.clone();
        let acceptor = if tls {
            use tokio_rustls::rustls;
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![rustls::pki_types::CertificateDer::from(CERT.to_vec())],
                    rustls::pki_types::PrivatePkcs8KeyDer::from(KEY.to_vec()).into(),
                )
                .unwrap();
            Some(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
        } else {
            None
        };
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let rows = output.clone();
                let closed = close_output.clone();
                let cases = cases.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Some(acceptor) = acceptor {
                        if let Ok(stream) = acceptor.accept(stream).await {
                            serve(stream, port, tls, cases, rows, closed).await;
                        }
                    } else {
                        serve(stream, port, tls, cases, rows, closed).await;
                    }
                });
            }
        });
        Self {
            port,
            rows,
            closed,
            task,
        }
    }
}
async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
fn json_wtf8(bytes: &[u8]) -> Value {
    let mut valid = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if index + 2 < bytes.len()
            && bytes[index] == 0xed
            && (0xa0..=0xbf).contains(&bytes[index + 1])
            && (0x80..=0xbf).contains(&bytes[index + 2])
        {
            let unit = (u16::from(bytes[index] & 15) << 12)
                | (u16::from(bytes[index + 1] & 63) << 6)
                | u16::from(bytes[index + 2] & 63);
            valid.extend(format!("\\u{unit:04x}").bytes());
            index += 3;
        } else {
            valid.push(bytes[index]);
            index += 1;
        }
    }
    // Keep lone units in an array before serde's scalar-string decoder sees them.
    let text = String::from_utf8(valid).unwrap();
    let key = if text.contains("\"script-sample\"") {
        "script-sample"
    } else {
        "sample"
    };
    let prefix = format!("\"{key}\":\"");
    let start = text.find(&prefix).unwrap() + prefix.len();
    let mut end = start;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if ch == '"' && !escaped {
            end = start + offset;
            break;
        }
        escaped = ch == '\\' && !escaped;
    }
    let sample = &text[start..end];
    let mut units = Vec::new();
    let mut chars = sample.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next().unwrap() {
                'u' => {
                    let code: String = chars.by_ref().take(4).collect();
                    units.push(u16::from_str_radix(&code, 16).unwrap());
                }
                'n' => units.push(10),
                'r' => units.push(13),
                't' => units.push(9),
                'b' => units.push(8),
                'f' => units.push(12),
                ch => units.extend(ch.to_string().encode_utf16()),
            }
        } else {
            units.extend(ch.to_string().encode_utf16());
        }
    }
    let replacement = format!("\"{key}Units\":{}", json!(units));
    let mut normalized = text[..start - prefix.len()].to_string();
    normalized.push_str(&replacement);
    normalized.push_str(&text[end + 1..]);
    serde_json::from_str(&normalized).unwrap()
}
fn normalize(value: Value, port: u16) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace(&format!(":{port}"), ":PORT")),
        Value::Array(rows) => Value::Array(rows.into_iter().map(|v| normalize(v, port)).collect()),
        Value::Object(rows) => Value::Object(
            rows.into_iter()
                .map(|(key, v)| (key, normalize(v, port)))
                .collect(),
        ),
        value => value,
    }
}
fn wire_projection(wire: &Wire, port: u16) -> Value {
    let row = wire;
    let body = if row.body.is_empty() {
        Value::Null
    } else {
        let mut body = json_wtf8(&row.body);
        if let Some(rows) = body.as_array_mut() {
            for row in rows {
                row["age"] = json!({"nonnegative":row["age"].as_u64().is_some()});
                row["user_agent"] = json!({"matchesWire":row["user_agent"].as_str()==wire.headers.get("user-agent").map(String::as_str)});
            }
        }
        body
    };
    normalize(
        json!({"url":row.path,"method":row.method,"body":body}),
        port,
    )
}
fn header_projection(headers: &BTreeMap<String, String>) -> Value {
    let names = [
        "content-type",
        "user-agent",
        "accept-language",
        "accept-encoding",
        "sec-ch-ua",
        "sec-ch-ua-mobile",
        "sec-ch-ua-platform",
        "priority",
        "access-control-request-method",
        "access-control-request-headers",
        "origin",
        "referer",
        "sec-fetch-site",
        "sec-fetch-mode",
        "sec-fetch-dest",
        "cache-control",
        "pragma",
        "accept",
    ];
    let mut value = serde_json::Map::new();
    for name in names {
        value.insert(
            name.into(),
            headers.get(name).map(|v| json!(v)).unwrap_or(Value::Null),
        );
    }
    value.insert(
        "sourceCookie".into(),
        json!(headers
            .get("cookie")
            .is_some_and(|value| value.split(';').any(|part| part.trim() == "source=1"))),
    );
    Value::Object(value)
}
#[tokio::test(flavor = "current_thread")]
async fn source_pinned_chrome_reports_use_actual_private_transport() {
    let _guard = SERIAL.lock().await;
    let cases: Vec<Value> = serde_json::from_str(CASES).unwrap();
    for tls in [false, true] {
        let server = Server::start(tls).await;
        let origin = format!(
            "{}://127.0.0.1:{}",
            if tls { "https" } else { "http" },
            server.port
        );
        let fixture: Value = serde_json::from_str(if tls {
            include_str!(
                "../../../../harness/detection/fixtures/csp-report-endpoint-https-chrome155.json"
            )
        } else {
            include_str!(
                "../../../../harness/detection/fixtures/csp-report-endpoint-chrome155.json"
            )
        })
        .unwrap();
        assert_eq!(fixture["browser"], "Chrome/155.0.8059.79");
        assert_eq!(fixture["v8"], "15.5.35.23");
        assert_eq!(fixture["errors"], json!([]));
        use sha2::{Digest, Sha256};
        for (key, text) in [
            ("casesSha256", CASES),
            ("probeSha256", PROBE),
            (
                "collectorSha256",
                include_str!("../../../../harness/detection/csp-report-endpoint-chrome.mjs"),
            ),
        ] {
            assert_eq!(
                fixture[key].as_str().unwrap(),
                format!("{:x}", Sha256::digest(text.as_bytes()))
            );
        }

        let mut storages = Vec::new();
        for (id, item) in cases.iter().enumerate() {
            if tls && item["frame"].as_bool() == Some(true) {
                continue;
            }
            let mut storage = BrowserStorage::default();
            if tls {
                storage.set_csp_report_transport_for_testing(
                    turbo_surf_core::net::report_client_with_trust(CERT).unwrap(),
                );
            }
            let effects = Arc::new(Mutex::new(Vec::<String>::new()));
            let output = effects.clone();
            storage.set_csp_report_response_effects(Arc::new(move |context, target, _| {
                assert!(context.sends_credentials(target));
                output.lock().unwrap().push(target.into());
            }));
            let is_frame = item["frame"].as_bool() == Some(true);
            let base = if is_frame {
                format!("{origin}/start?case={id}&source=authored#referrer")
            } else {
                format!("{origin}/page?case={id}&private=authored#fragment")
            };
            let page_headers = if is_frame {
                vec![("Referrer-Policy".into(), "unsafe-url".into())]
            } else {
                headers(item, id, server.port, tls)
            };
            let document = if is_frame {
                "<!doctype html><body>owner</body>".into()
            } else {
                html(item, id, server.port, tls)
            };
            let mut seed = turbo_surf_core::cookies::CookieJar::new();
            seed.add("source", "1", "127.0.0.1", "/", None);
            seed.add("cross", "2", "localhost", "/", None);
            let mut page = PageSession::open_with_document_response(
                &document,
                &base,
                &seed.storage_state(),
                "",
                10000,
                "",
                ClientHints::default(),
                &page_headers,
                storage.clone(),
                DocumentResponseMetadata {
                    status: 200,
                    referrer: format!("{origin}/start?case={id}&source=authored"),
                },
            )
            .await
            .unwrap();
            if is_frame {
                page.eval(&format!("(()=>{{const frame=document.createElement('iframe');frame.id='frame';frame.src={};document.body.append(frame);}})()",json!(format!("{origin}/page?case={id}&private=authored#fragment")))).await.unwrap();
                assert_eq!(
                    page.eval_expression(
                        "document.getElementById('frame').contentWindow.location.pathname"
                    )
                    .await
                    .unwrap(),
                    "/page"
                );
                page.eval(&format!("(()=>{{const child=document.getElementById(\"frame\");child.contentDocument.body.setAttribute(\"style\",\"color:red\");child.remove();}})()\n//# sourceURL={origin}/csp-report-endpoint-probe.js")).await.unwrap();
                page.collect_message_port_garbage_for_testing();
            } else {
                let mut input = item.clone();
                input["meta"] = Value::Null;
                input["id"] = json!(id);
                page.eval(&format!(
                    "({})({})\n//# sourceURL={origin}/csp-report-endpoint-probe.js",
                    PROBE.trim().trim_end_matches(';'),
                    input
                ))
                .await
                .unwrap();
            }
            let expected = fixture["wire"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["case"] == id)
                .collect::<Vec<_>>();
            let is_hold = item["name"] == "frame-hold-gc";
            if is_hold {
                until(|| {
                    server
                        .rows
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|row| row.path.contains(&format!("case={id}")))
                })
                .await;
            } else {
                until(|| storage.csp_report_diagnostics_for_testing()["jobs"] == 0).await;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
            let actual = server
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|row| {
                    url::Url::parse(&format!("http://localhost{}", row.path))
                        .unwrap()
                        .query_pairs()
                        .any(|(key, value)| key == "case" && value == id.to_string())
                })
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(actual.len(), expected.len(), "{} tls={tls}", item["name"]);
            for (actual, expected) in actual.iter().zip(&expected) {
                assert_eq!(
                    wire_projection(actual, server.port),
                    json!({"url":expected["url"],"method":expected["method"],"body":expected["body"]}),
                    "{} tls={tls}",
                    item["name"]
                );
                let expected_headers: BTreeMap<String, String> = expected["headers"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_string()))
                    })
                    .collect();
                assert_eq!(
                    normalize(header_projection(&actual.headers), server.port),
                    header_projection(&expected_headers),
                    "{} tls={tls}",
                    item["name"]
                );
            }
            if !is_hold {
                let admitted = expected
                    .iter()
                    .filter(|row| row["headers"]["cookie"].is_string())
                    .count();
                assert_eq!(
                    effects.lock().unwrap().len(),
                    admitted,
                    "raw admission {}",
                    item["name"]
                );
            }
            if !is_hold
                && !is_frame
                && expected.iter().any(|row| {
                    row["method"] == "POST"
                        && row["headers"]["cookie"].is_string()
                        && row["url"].as_str().unwrap().starts_with("/report")
                })
            {
                assert!(
                    page.eval_expression("document.cookie")
                        .await
                        .unwrap()
                        .contains(&format!("report_response={id}")),
                    "{}",
                    item["name"]
                );
            }
            if !is_frame {
                assert_eq!(
                    page.eval_expression("reportEventCount")
                        .await
                        .unwrap()
                        .parse::<u64>()
                        .unwrap(),
                    fixture["result"][id]["eventCount"].as_u64().unwrap(),
                    "DOM events {}",
                    item["name"]
                );
            }
            page.close();
            storages.push(storage);
        }
        drop(storages);
        if !tls {
            until(|| {
                server
                    .closed
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|path| path == "/hold-report?case=17")
            })
            .await;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn stalled_reports_survive_frame_retirement_without_realm_roots_and_end_with_profile() {
    let _guard = SERIAL.lock().await;
    let server = Server::start(false).await;
    let origin = format!("http://127.0.0.1:{}", server.port);
    let storage = BrowserStorage::default();
    let mut page = PageSession::open_with_browsing_state(
        "<!doctype html><body></body>",
        &format!("{origin}/start"),
        "",
        "",
        10000,
        "",
        ClientHints::default(),
        &[],
        storage.clone(),
    )
    .await
    .unwrap();
    page.eval(&format!("globalThis.frame=document.createElement('iframe');frame.src={};document.body.append(frame)",json!(format!("{origin}/page?case=17")))).await.unwrap();
    page.eval(
        "frame.contentDocument.body.setAttribute('style','color:red');frame.remove();frame=null;",
    )
    .await
    .unwrap();
    until(|| !server.rows.lock().unwrap().is_empty()).await;
    for _ in 0..6 {
        page.collect_message_port_garbage_for_testing();
        page.eval("void 0").await.unwrap();
    }
    assert_eq!(storage.csp_report_diagnostics_for_testing()["jobs"], 1);
    assert!(server.closed.lock().unwrap().is_empty());
    let diagnostics = page.frame_lifecycle_diagnostics_for_testing();
    assert_eq!(diagnostics["retiredHostContexts"], 0, "{diagnostics}");
    page.close();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(server.closed.lock().unwrap().is_empty());
    drop(storage);
    until(|| !server.closed.lock().unwrap().is_empty()).await;
}
