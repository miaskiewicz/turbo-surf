//! Child-frame form navigations beyond the submitting document's own frames: the
//! submitter's `form-action` on every redirect hop (turbo-surf#139) and target names
//! resolved over the whole frame tree (turbo-surf#140), against the Chrome 156
//! control, over a localhost server mirroring the control's routes. `localhost` and
//! `127.0.0.1` serve as two origins of the same server.
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const REDIRECTS: &str =
    include_str!("../../../../harness/detection/child-frame-form-redirects-probe.js");
const TARGETS: &str =
    include_str!("../../../../harness/detection/child-frame-form-targets-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/child-frame-form-targets-chrome156.json");

fn control(name: &str, probe: &str) -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .ends_with("Chrome/156.0.8078.47"));
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(
        control["fixtures"][name],
        format!("{:x}", Sha256::digest(probe)),
        "stale control"
    );
    control["result"][name].clone()
}

/// JSON-pointer paths where `actual` departs from Chrome, with the engine's value.
fn divergences(path: String, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Object(left), Value::Object(right)) => {
            let mut keys: Vec<_> = left.keys().chain(right.keys()).collect();
            keys.sort();
            keys.dedup();
            let missing = Value::String("<absent>".into());
            for key in keys {
                divergences(
                    format!("{path}/{key}"),
                    left.get(key).unwrap_or(&missing),
                    right.get(key).unwrap_or(&missing),
                    out,
                );
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                divergences(format!("{path}/{index}"), left, right, out);
            }
        }
        _ if actual != expected => out.push(format!("{path} = {actual}")),
        _ => {}
    }
}

const ECHO_HEADERS: [&str; 10] = [
    "content-type",
    "content-length",
    "origin",
    "referer",
    "cache-control",
    "upgrade-insecure-requests",
    "sec-fetch-dest",
    "sec-fetch-mode",
    "sec-fetch-site",
    "sec-fetch-user",
];

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

fn parse(head: &str) -> Request {
    let mut lines = head.split("\r\n");
    let mut start = lines.next().unwrap_or("").split(' ');
    Request {
        method: start.next().unwrap_or("").to_string(),
        path: start.next().unwrap_or("/").to_string(),
        headers: lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
            .collect(),
    }
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.as_str())
    }
    fn query(&self, name: &str) -> String {
        url::Url::parse(&format!("http://host{}", self.path))
            .ok()
            .and_then(|url| {
                url.query_pairs()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.into_owned())
            })
            .unwrap_or_default()
    }
}

fn echo(request: &Request, body: &[u8]) -> String {
    let origin = format!("http://{}", request.header("host").unwrap_or(""));
    let boundary = request
        .header("content-type")
        .and_then(|value| value.split_once("boundary="))
        .map(|(_, boundary)| boundary.to_string());
    let scrub = |text: &str| {
        let text = text.replace(&origin, "<origin>");
        boundary.as_ref().map_or(text.clone(), |boundary| {
            text.replace(boundary, "<boundary>")
        })
    };
    let mut echoed = serde_json::Map::new();
    for name in ECHO_HEADERS {
        if let Some(value) = request.header(name) {
            echoed.insert(name.into(), scrub(value).into());
        }
    }
    let body: String = body.iter().map(|byte| char::from(*byte)).collect();
    let echo = serde_json::json!({"method": request.method, "url": request.path, "headers": echoed, "body": scrub(&body)});
    let text = echo.to_string().replace('&', "&amp;").replace('<', "&lt;");
    format!("<!doctype html><html><head></head><body><pre id=\"echo\">{text}</pre></body></html>")
}

/// A page whose script submits a POST form to each `[target, action]` pair.
fn submitter(targets: &str) -> String {
    format!(
        "<!doctype html><html><head></head><body><script>for (const [target, action] of {targets}) {{
        const form = document.createElement(\"form\");
        form.method = \"post\"; form.action = action; form.target = target;
        document.body.append(form);
        try {{ form.submit(); }} catch (_) {{}}
      }}</script></body></html>"
    )
}

/// The control server's routes (`web-apis-chrome-control.mjs`) as a full response.
fn respond(head: &str, body: &[u8]) -> String {
    let request = parse(head);
    let path = request.path.as_str();
    if let Some(status) = path
        .strip_prefix("/redirect")
        .and_then(|rest| rest.get(..3))
        .filter(|_| path.as_bytes().get(12) == Some(&b'?'))
    {
        let location = request.query("to");
        return format!(
            "HTTP/1.1 {status} Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    }
    let html = if path.starts_with("/xo-submit?") {
        submitter(&request.query("targets"))
    } else if path.starts_with("/echo") {
        echo(&request, body)
    } else {
        let directive = if path.starts_with("/csp-") {
            "'none'"
        } else if path.starts_with("/self-csp") {
            "'self'"
        } else {
            ""
        };
        let policy = if directive.is_empty() {
            String::new()
        } else {
            format!(
                "<meta http-equiv=\"Content-Security-Policy\" content=\"form-action {directive}\">"
            )
        };
        format!("<!doctype html><html><head>{policy}</head><body></body></html>")
    };
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    )
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<(String, Vec<u8>)> {
    let mut request = Vec::new();
    let mut chunk = [0; 4096];
    let split = loop {
        if let Some(at) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            break at;
        }
        let size = socket.read(&mut chunk).await.unwrap_or(0);
        if size == 0 {
            return None;
        }
        request.extend_from_slice(&chunk[..size]);
    };
    let head = String::from_utf8_lossy(&request[..split]).to_string();
    let length = parse(&head)
        .header("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    while request.len() < split + 4 + length {
        let size = socket.read(&mut chunk).await.unwrap_or(0);
        if size == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..size]);
    }
    let end = request.len().min(split + 4 + length);
    Some((head, request[split + 4..end].to_vec()))
}

async fn serve(listener: tokio::net::TcpListener) {
    while let Ok((mut socket, _)) = listener.accept().await {
        tokio::spawn(async move {
            if let Some((head, body)) = read_request(&mut socket).await {
                let _ = socket.write_all(respond(&head, &body).as_bytes()).await;
            }
        });
    }
}

async fn page(port: u16) -> PageSession {
    PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("http://127.0.0.1:{port}/"),
        "",
        "",
        // ~20 frame loads and navigations over localhost; headroom for a loaded host.
        60000,
    )
    .await
    .unwrap()
}

/// Runs one probe in a fresh session (each keeps its frame count near the
/// existing child-frame form control's) and compares it with its Chrome control.
async fn matches_chrome156(name: &str, probe: &str) {
    let expected = control(name, probe);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(serve(listener));
    let mut page = page(port).await;
    let raw = page
        .eval_json_expression(probe.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let value: Value = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
    let mut found = Vec::new();
    divergences(String::new(), &actual, &expected, &mut found);
    assert_eq!(
        found,
        Vec::<String>::new(),
        "divergences from the Chrome control"
    );
}

/// turbo-surf#139: a refused hop cancels the navigation (no load, the document
/// stays) and the submitter reports the violation against the initial URL.
#[tokio::test(flavor = "current_thread")]
async fn child_frame_form_redirects_match_chrome156() {
    matches_chrome156("child-frame-form-redirects-probe.js", REDIRECTS).await;
}

/// turbo-surf#140: target names over the whole frame tree.
#[tokio::test(flavor = "current_thread")]
async fn child_frame_form_targets_match_chrome156() {
    matches_chrome156("child-frame-form-targets-probe.js", TARGETS).await;
}

/// `_blank` and names no navigable answers to open a new top-level window in Chrome;
/// a no-browser engine has none, so submission throws instead of silently dropping
/// the navigation (turbo-surf#140).
#[tokio::test(flavor = "current_thread")]
async fn new_window_targets_throw_in_a_no_browser_engine() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(serve(listener));
    let mut page = page(port).await;
    let raw = page
        .eval_json_expression(
            r#"(async()=>{
  const loaded=(el)=>new Promise(r=>el.addEventListener('load',r,{once:true}));
  const frame=async(src,name)=>{const el=document.createElement('iframe');if(name)el.name=name;el.src=src;const ready=loaded(el);document.body.append(el);await ready;return el;};
  const attempt=(doc,target)=>{const form=doc.createElement('form');form.method='post';form.action='/echo/new';form.target=target;doc.body.append(form);try{form.submit();return 'submitted';}catch(e){return e.name+': '+e.message;}};
  const child=await frame('/child');
  const out={blank:attempt(document,'_blank'),unknown:attempt(document,'nowhere'),childUnknown:attempt(child.contentDocument,'nowhere')};
  return JSON.stringify(out);
})()"#,
        )
        .await
        .unwrap();
    page.close();
    let value: Value = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
    let thrown = "TypeError: Native form submission to another browsing context is not supported";
    assert_eq!(
        actual,
        serde_json::json!({"blank": thrown, "unknown": thrown, "childUnknown": thrown})
    );
}
