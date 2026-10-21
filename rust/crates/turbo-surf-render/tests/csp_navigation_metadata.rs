use sha2::{Digest, Sha256};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::{BrowserStorage, DocumentResponseMetadata, PageSession};

const PROBE: &str = include_str!("../../../../harness/detection/csp-navigation-probe.js");
const CASES: &str =
    include_str!("../../../../harness/detection/fixtures/csp-navigation-cases.json");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/csp-navigation-chrome155.json");

#[test]
fn navigation_metadata_control_pins_actual_independent_sources() {
    let control: serde_json::Value = serde_json::from_str(CONTROL).unwrap();
    for (key, bytes) in [
        ("probeSha256", PROBE.as_bytes()),
        ("casesSha256", CASES.as_bytes()),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/csp-navigation-chrome.mjs").as_slice(),
        ),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", Sha256::digest(bytes)),
            "{key}"
        );
    }
    assert_eq!(control["errors"], serde_json::json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn authentic_root_navigation_status_and_referrer_survive_guest_getter_overrides() {
    let cases: serde_json::Value = serde_json::from_str(CASES).unwrap();
    let control: serde_json::Value = serde_json::from_str(CONTROL).unwrap();
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        if case.get("frame").is_some() {
            continue;
        }
        // Navigation inputs come from the collector's actual response and HTTP
        // request; event/referrer outputs remain independently measured values.
        let request_referrer = control["result"][index]["request"]["referrer"]
            .as_str()
            .unwrap();
        let metadata = DocumentResponseMetadata {
            status: case["status"].as_u64().unwrap() as u16,
            referrer: request_referrer.to_string(),
        };
        let mut page = PageSession::open_with_document_response(
            "<!doctype html><head></head><body></body>",
            &format!(
                "http://metadata.test/page?case={index}{}",
                if case["fragment"].as_bool().unwrap_or(false) {
                    "#authored"
                } else {
                    ""
                }
            ),
            "",
            "",
            15000,
            "",
            ClientHints::default(),
            &[("Content-Security-Policy".into(), "style-src 'none'".into())],
            BrowserStorage::default(),
            metadata,
        )
        .await
        .unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&page.eval_json_expression(PROBE).await.unwrap()).unwrap();
        assert_eq!(result, control["result"][index]["result"], "{}", case["id"]);
        page.close();
    }
}

pub(crate) struct FrameServer {
    listener: std::net::TcpListener,
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl FrameServer {
    fn start() -> Self {
        Self::start_cases(CASES)
    }
    pub(crate) fn start_cases(cases: &str) -> Self {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let worker_listener = listener.try_clone().unwrap();
        let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        let cases: serde_json::Value = serde_json::from_str(cases).unwrap();
        let source = relay_source();
        let worker = std::thread::spawn(move || {
            while !worker_stopped.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = worker_listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    continue;
                };
                // macOS accept inherits the listener's nonblocking flag. The
                // complete-header reader must wait instead of dropping an early socket.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 2048];
                while request.len() < 16384
                    && !request.windows(4).any(|window| window == b"\r\n\r\n")
                {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => request.extend_from_slice(&chunk[..count]),
                    }
                }
                if !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    continue;
                }
                let text = String::from_utf8_lossy(&request);
                let Some(path) = text
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                else {
                    continue;
                };
                let parsed = url::Url::parse(&format!("http://fixture.test{path}")).unwrap();
                if parsed.path() == "/echo" {
                    let referrer = text
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("referer").then(|| value.trim())
                        })
                        .unwrap_or("");
                    let body = serde_json::json!({
                        "slot": parsed.query_pairs().find(|(key, _)| key == "slot").map(|(_, value)| value.into_owned()),
                        "referrer": referrer,
                        "method": text.split_whitespace().next().unwrap_or("")
                    }).to_string();
                    let response = format!("HTTP/1.1 200 fixture\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = stream.write_all(response.as_bytes());
                    continue;
                }
                let index = parsed
                    .query_pairs()
                    .find(|(key, _)| key == "case")
                    .and_then(|(_, value)| value.parse::<usize>().ok())
                    .unwrap_or(0);
                let status = cases[index]["status"].as_u64().unwrap_or(200);
                let body =
                    format!("<!doctype html><head></head><body><script>{source}</script></body>");
                let response = format!("HTTP/1.1 {status} fixture\r\nContent-Type: text/html\r\nContent-Security-Policy: style-src 'none'\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            listener,
            stopped,
            worker: Some(worker),
        }
    }
    pub(crate) fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/",
            self.listener.local_addr().unwrap().port()
        )
    }
}
impl Drop for FrameServer {
    fn drop(&mut self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn relay_source() -> String {
    PROBE.replace(
        "})();",
        "})().then(result=>parent.postMessage(JSON.stringify(result),'*'));",
    )
}

#[tokio::test(flavor = "current_thread")]
async fn authentic_frame_metadata_matches_normal_chrome_without_guest_document_reads() {
    let server = FrameServer::start();
    let base = server.url();
    let cross = base.replace("127.0.0.1", "localhost");
    let cases: serde_json::Value = serde_json::from_str(CASES).unwrap();
    let control: serde_json::Value = serde_json::from_str(CONTROL).unwrap();
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        let Some(kind) = case["frame"].as_str() else {
            continue;
        };
        let policy = case["policy"]
            .as_str()
            .unwrap_or("strict-origin-when-cross-origin");
        let owner = format!("{base}start?case={index}&private=authored#fragment");
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><head></head><body></body>",
            &owner,
            "",
            "",
            15000,
            "",
            ClientHints::default(),
            &[
                ("Content-Security-Policy".into(), "style-src 'none'".into()),
                ("Referrer-Policy".into(), policy.into()),
            ],
        )
        .await
        .unwrap();
        let child = format!(
            "{}page?case={index}{}",
            if case["child"] == "cross" {
                &cross
            } else {
                &base
            },
            if case["fragment"].as_bool().unwrap_or(false) {
                "#authored"
            } else {
                ""
            }
        );
        let html = format!(
            "<!doctype html><head></head><body><script>{}</script></body>",
            relay_source()
        );
        let options =
            serde_json::json!({"kind":kind,"child":child,"source":relay_source(),"html":html,"framePolicy":case.get("framePolicy")})
                .to_string();
        let script = format!(
            r#"new Promise(resolve=>{{const options={options};addEventListener('message',event=>{{if(typeof event.data==='string')resolve(JSON.parse(event.data))}},{{once:true}});const frame=document.createElement('iframe');if(options.framePolicy!==null)frame.setAttribute('referrerpolicy',options.framePolicy);Object.defineProperty(frame,'referrerPolicy',{{get(){{throw new Error('guest referrerPolicy getter')}}}});if(options.kind==='http')frame.src=options.child;else if(options.kind==='srcdoc')frame.srcdoc=options.html;else if(options.kind==='data')frame.src='data:text/html,'+encodeURIComponent(options.html);else frame.onload=()=>{{const script=frame.contentDocument.createElement('script');script.textContent=options.source;frame.contentDocument.head.append(script)}};document.body.append(frame)}})"#
        );
        let collected = page.eval_json_expression(&script).await;
        if let Err(error) = &collected {
            eprintln!(
                "{}: {error}; native requests {}; frames {}",
                case["id"],
                page.drain_network_log(),
                page.frame_lifecycle_diagnostics_for_testing()
            );
        }
        let result = collected
            .unwrap()
            .replace(&base, "http://metadata.test/")
            .replace(&cross, "http://cross.metadata.test/");
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        if result != control["result"][index]["result"] {
            eprintln!("native requests {}", page.drain_network_log());
        }
        assert_eq!(result, control["result"][index]["result"], "{}", case["id"]);
        page.close();
    }
}
