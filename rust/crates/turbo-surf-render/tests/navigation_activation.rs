use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use turbo_surf_render::{
    AuxiliaryBootstrap, AuxiliaryHostServices, BrowserStorage, DocumentResponseMetadata,
    PageSession, PageSessionOptions,
};

static ORIGIN_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const STATE: &str =
    "({active:navigator.userActivation.isActive,sticky:navigator.userActivation.hasBeenActive})";

struct Destination {
    origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn request_path(stream: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    while bytes.len() < 16384 {
        let remaining = (16384 - bytes.len()).min(chunk.len());
        let count = stream.read(&mut chunk[..remaining]).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|end| end == b"\r\n\r\n") {
            return std::str::from_utf8(&bytes)
                .ok()?
                .split_whitespace()
                .nth(1)
                .map(str::to_owned);
        }
    }
    None
}

impl Destination {
    fn start(opaque: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let worker_stop = stop.clone();
        let worker_requests = requests.clone();
        let thread = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                        let Some(path) = request_path(&mut stream) else {
                            continue;
                        };
                        worker_requests.lock().unwrap().push(path);
                        let body = format!("<script>globalThis.activationAtParse={STATE}</script>");
                        let sandbox = if opaque {
                            "Content-Security-Policy: sandbox allow-scripts\r\n"
                        } else {
                            ""
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n{sandbox}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            origin,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Destination {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn destination_state(page: &mut PageSession, destination: &Destination) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            page.poll_root_navigation_work().await.unwrap();
            let value = page
                .eval_json_expression(&format!(
                    "globalThis.activationAtParse ? {{init:activationAtInit,parser:activationAtParse,current:{STATE}}} : null"
                ))
                .await
                .unwrap();
            let value: Value = serde_json::from_str(&value).unwrap();
            if !value.is_null() {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap_or_else(|error| {
        panic!(
            "the real destination parser must execute: {error}; wire={:?}",
            destination.requests.lock().unwrap()
        )
    })
}

// Host origin policy is process-immutable, so each case configures it before first use.
fn isolated_case(name: &str) -> bool {
    const CHILD: &str = "TURBO_SURF_NAVIGATION_ACTIVATION_CASE";
    if std::env::var(CHILD).ok().as_deref() == Some(name) {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD, name)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{name}: {stdout}\n{stderr}");
    assert!(stdout.contains("running 1 test"), "{name}: {stdout}");
    assert!(
        stdout.contains("1 passed; 0 failed; 0 ignored"),
        "{name}: {stdout}\n{stderr}"
    );
    true
}

async fn navigation_case(cross_origin: bool, source_opaque: bool, destination_opaque: bool) {
    let _environment = ORIGIN_ENV.lock().await;
    let primary = Destination::start(destination_opaque);
    let secondary = Destination::start(destination_opaque);
    let destination = if cross_origin { &secondary } else { &primary };
    std::env::set_var(
        "TURBO_SURF_ALLOWED_ORIGINS",
        serde_json::to_string(&[&primary.origin, &secondary.origin]).unwrap(),
    );
    let source_url = format!("{}/start", primary.origin);
    let target = format!("{}/destination", destination.origin);
    let html = format!(
        "<button id=go onclick=\"location.assign('{target}?trusted='+event.isTrusted+'&active='+navigator.userActivation.isActive+'&sticky='+navigator.userActivation.hasBeenActive)\">Go</button>"
    );
    let headers = if source_opaque {
        vec![(
            "content-security-policy".to_owned(),
            "sandbox allow-scripts".to_owned(),
        )]
    } else {
        Vec::new()
    };
    let init = format!("globalThis.activationAtInit={STATE}");
    let services = AuxiliaryHostServices {
        root_bootstrap: AuxiliaryBootstrap {
            init_script: init.clone(),
            engine_init_script: String::new(),
        },
        ..Default::default()
    };
    let mut page = PageSession::open_with_document_response_and_auxiliary_services(
        &html,
        &source_url,
        "",
        "",
        10000,
        &init,
        Default::default(),
        &headers,
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions::default(),
        "",
        None,
        services,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().expect("actual native root");
    page.input(&json!([{"click":"#go"}])).await.unwrap();
    let actual = destination_state(&mut page, destination).await;
    assert_eq!(page.auxiliary_root_id().unwrap(), root);
    let paths = destination.requests.lock().unwrap().clone();
    let request = paths
        .iter()
        .find(|path| path.starts_with("/destination?"))
        .expect("trusted input must navigate the actual Page");
    let request = url::Url::parse(&format!("{}{}", destination.origin, request)).unwrap();
    let query = request
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(query.get("trusted").unwrap(), "true");
    assert_eq!(query.get("active").unwrap(), "true");
    assert_eq!(query.get("sticky").unwrap(), "true");
    let expected =
        json!({"active":false,"sticky":!cross_origin && !source_opaque && !destination_opaque});
    assert_eq!(
        actual,
        json!({"init":expected,"parser":expected,"current":expected})
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn same_origin_replacement_retains_only_sticky_before_init_and_parser() {
    if isolated_case("same_origin_replacement_retains_only_sticky_before_init_and_parser") {
        return;
    }
    navigation_case(false, false, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn cross_origin_replacement_has_no_activation_before_init_and_parser() {
    if isolated_case("cross_origin_replacement_has_no_activation_before_init_and_parser") {
        return;
    }
    navigation_case(true, false, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn opaque_source_cannot_transfer_sticky_activation_to_matching_url_origin() {
    if isolated_case("opaque_source_cannot_transfer_sticky_activation_to_matching_url_origin") {
        return;
    }
    navigation_case(false, true, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn opaque_destination_cannot_inherit_sticky_from_matching_url_origin() {
    if isolated_case("opaque_destination_cannot_inherit_sticky_from_matching_url_origin") {
        return;
    }
    navigation_case(false, false, true).await;
}
