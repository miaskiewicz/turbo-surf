//! Real classic-script parse failures exercise the native Page compatibility module graph.
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn module_body(path: &str) -> Option<&'static str> {
    match path {
        "/dep.mjs" => Some("export const value=42;"),
        "/entry.js" => Some("import {value} from './dep.mjs';globalThis.compat={value,id:document.currentScript.id};"),
        "/parse.js" => Some("import './dep.mjs';export const broken=;"),
        "/runtime.js" => Some("import './dep.mjs';throw new Error('compat-runtime-failure');"),
        "/load.js" => Some("import './missing.mjs';"),
        "/recovery.js" => Some("export const recovered=true;globalThis.recovery={recovered,id:document.currentScript.id};"),
        _ => None,
    }
}

async fn serve_connection(mut socket: tokio::net::TcpStream, requests: Arc<Mutex<Vec<String>>>) {
    let mut request = Vec::new();
    loop {
        let mut bytes = [0; 4096];
        let Ok(count) = socket.read(&mut bytes).await else {
            return;
        };
        if count == 0 {
            return;
        }
        request.extend_from_slice(&bytes[..count]);
        if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&request);
    let path = text
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap();
    requests.lock().unwrap().push(path.to_owned());
    let (status, body) = module_body(path).map_or(
        ("404 Not Found", "globalThis.missingModuleExecuted=true;"),
        |body| ("200 OK", body),
    );
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

async fn server() -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let task = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve_connection(socket, observed.clone()));
        }
    });
    Server {
        base,
        requests,
        task,
    }
}

fn assert_current_script_cleared(page: &mut PageSession) {
    let root = page.auxiliary_root_id().unwrap();
    assert_eq!(
        page.read_target_engine(root, "return document.currentScript===null")
            .unwrap(),
        "true"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn inline_classic_export_uses_the_owned_compatibility_module_context() {
    let mut page = PageSession::open("<body><script id=compat>export const value=42;globalThis.compat={value,id:document.currentScript.id};</script>", "https://classic-module.test/", "", "", 5000).await.unwrap();
    assert!(!page.hydration_diagnostics().budget_exceeded);
    assert_eq!(
        page.eval_json_expression("compat").await.unwrap(),
        r#"{"value":42,"id":"compat"}"#
    );
    assert_eq!(
        page.eval_json_expression("typeof value").await.unwrap(),
        "\"undefined\""
    );
    assert_current_script_cleared(&mut page);
}

#[tokio::test(flavor = "current_thread")]
async fn external_classic_module_body_links_relative_imports_without_refetching_entry() {
    let server = server().await;
    let mut page = PageSession::open(
        "<body><script id=compat src='/entry.js'></script>",
        &server.base,
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    assert!(!page.hydration_diagnostics().budget_exceeded);
    assert_eq!(
        page.eval_json_expression("compat").await.unwrap(),
        r#"{"value":42,"id":"compat"}"#
    );
    assert_current_script_cleared(&mut page);
    let requests = server.requests.lock().unwrap();
    for path in ["/entry.js", "/dep.mjs"] {
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.as_str() == path)
                .count(),
            1,
            "{requests:?}"
        );
    }
}

async fn assert_failed_module_cleans_current_script(path: &str, expected: &str) {
    let server = server().await;
    let mut page = PageSession::open("<body>", &server.base, "", "", 5000)
        .await
        .unwrap();
    let error = page
        .set_content(
            &format!("<body><script id=failed src='{path}'></script>"),
            "load",
            None,
        )
        .await
        .unwrap_err();
    assert!(error.contains(expected), "{error}");
    assert_eq!(
        page.eval_json_expression("globalThis.missingModuleExecuted===undefined")
            .await
            .unwrap(),
        "true"
    );
    assert_current_script_cleared(&mut page);
    page.set_content(
        "<body><script id=recovery src='/recovery.js'></script>",
        "load",
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        page.eval_json_expression("recovery").await.unwrap(),
        r#"{"recovered":true,"id":"recovery"}"#
    );
    assert_current_script_cleared(&mut page);
}

#[tokio::test(flavor = "current_thread")]
async fn compatibility_module_parse_failure_cleans_current_script_and_allows_recovery() {
    assert_failed_module_cleans_current_script("/parse.js", "SyntaxError").await;
}

#[tokio::test(flavor = "current_thread")]
async fn compatibility_module_load_failure_cleans_current_script_and_allows_recovery() {
    assert_failed_module_cleans_current_script("/load.js", "HTTP 404").await;
}

#[tokio::test(flavor = "current_thread")]
async fn compatibility_module_runtime_failure_cleans_current_script_and_allows_recovery() {
    assert_failed_module_cleans_current_script("/runtime.js", "compat-runtime-failure").await;
}

#[tokio::test(flavor = "current_thread")]
async fn import_export_words_in_classic_comments_and_strings_do_not_create_a_module() {
    let mut page = PageSession::open("<body><script id=ordinary>/* import value from './missing.mjs'; export const ignored=1; */globalThis.classic={text:'import export',receiver:this===window,id:document.currentScript.id};globalThis.classicRuns=(globalThis.classicRuns||0)+1;</script>", "https://classic-module.test/", "", "", 5000).await.unwrap();
    assert_eq!(
        page.eval_json_expression("classic").await.unwrap(),
        r#"{"text":"import export","receiver":true,"id":"ordinary"}"#
    );
    assert_eq!(page.eval_json_expression("classicRuns").await.unwrap(), "1");
    assert_current_script_cleared(&mut page);
}
