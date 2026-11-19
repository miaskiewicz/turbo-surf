use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use turbo_surf_core::net::{self, FetchOptions};
use turbo_surf_core::websocket::{self, WsEvent, WsRequest};

struct Server {
    origin: String,
    accepts: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(redirect: Option<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let accepts = Arc::new(AtomicUsize::new(0));
        let count = accepts.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                count.fetch_add(1, Ordering::SeqCst);
                let redirect = redirect.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        if socket.read_exact(&mut byte).await.is_err() {
                            return;
                        }
                        head.push(byte[0]);
                    }
                    let head = String::from_utf8(head).unwrap();
                    let response = if head.starts_with("GET /redirect ") {
                        format!("HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", redirect.unwrap())
                    } else if let Some(key) = head.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("sec-websocket-key")
                            .then(|| value.trim())
                    }) {
                        format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n", websocket::accept_key(key))
                    } else {
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nCache-Control: max-age=3600\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_owned()
                    };
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            origin,
            accepts,
            task,
        }
    }
}

async fn probe(mode: &str) {
    let denied = Server::start(None).await;
    let allowed = Server::start(Some(format!(
        "{}/secret?key=must-not-appear",
        denied.origin
    )))
    .await;
    match mode {
        "absent" => std::env::remove_var("TURBO_SURF_ALLOWED_ORIGINS"),
        "enabled" => std::env::set_var(
            "TURBO_SURF_ALLOWED_ORIGINS",
            serde_json::to_string(&[&allowed.origin, &allowed.origin.replace("http:", "ws:")])
                .unwrap(),
        ),
        #[cfg(unix)]
        "nonunicode" => {
            use std::os::unix::ffi::OsStringExt;
            std::env::set_var(
                "TURBO_SURF_ALLOWED_ORIGINS",
                std::ffi::OsString::from_vec(vec![0xff]),
            );
        }
        "empty" => std::env::set_var("TURBO_SURF_ALLOWED_ORIGINS", "[]"),
        "malformed" => std::env::set_var("TURBO_SURF_ALLOWED_ORIGINS", "not-json"),
        "path" => std::env::set_var(
            "TURBO_SURF_ALLOWED_ORIGINS",
            format!("[\"{}/path\"]", allowed.origin),
        ),
        _ => unreachable!(),
    }
    let enabled = mode != "absent";
    let allowed_request = net::fetch_html(&allowed.origin, FetchOptions::default()).await;
    assert_eq!(
        allowed_request.is_ok(),
        matches!(mode, "enabled" | "absent"),
        "allowed origin mode={mode}: {allowed_request:?}"
    );
    let denied_request = net::fetch_html(&denied.origin, FetchOptions::default()).await;
    assert_eq!(
        denied_request.is_err(),
        enabled,
        "unapproved origin contacted in mode={mode}"
    );
    if mode == "enabled" {
        let error = net::fetch_html(
            &format!("{}/redirect", allowed.origin),
            FetchOptions::default(),
        )
        .await
        .unwrap_err();
        assert!(!error.to_string().contains("must-not-appear"));
        assert!(net::fetch_bytes(
            &format!("{}/redirect", allowed.origin),
            FetchOptions::default()
        )
        .await
        .is_err());
        assert!(net::fetch_stream(
            &format!("{}/redirect", allowed.origin),
            FetchOptions::default()
        )
        .await
        .is_err());
        use turbo_surf_core::challenge::{Challenge, ChallengeSolver, SolveContext, Vendor};
        let challenge = Challenge::new(Vendor::Cloudflare, &denied.origin);
        let solvers: Vec<Box<dyn ChallengeSolver>> = vec![
            Box::new(
                turbo_surf_core::challenge::ScrapflySolver::new("disposable".into())
                    .with_base_url(denied.origin.clone()),
            ),
            Box::new(
                turbo_surf_core::challenge::HyperSolver::new("disposable".into())
                    .with_base_url(denied.origin.clone()),
            ),
            Box::new(turbo_surf_core::challenge::BrowserSolver::new(
                "exit 0".into(),
            )),
            Box::new(turbo_surf_core::cloudflare::CloudflareSolver::new()),
            Box::new(turbo_surf_core::akamai::AkamaiSolver::new()),
            Box::new(turbo_surf_core::aws_waf::AwsWafSolver::new()),
            Box::new(turbo_surf_core::recaptcha::RecaptchaSolver::new()),
        ];
        for solver in solvers {
            let error = solver
                .solve(&challenge, &SolveContext::default())
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("disabled by host network policy"),
                "{}: {error}",
                solver.name()
            );
        }
        assert!(turbo_surf_core::challenge::solver_from_env().is_none());
        let report_client = net::build_report_client();
        assert!(net::send_report_request(
            &report_client,
            "POST",
            &denied.origin,
            &BTreeMap::new(),
            vec![],
            false
        )
        .await
        .is_err());
        assert!(net::send_report_request(
            &report_client,
            "POST",
            &denied.origin,
            &BTreeMap::new(),
            vec![],
            true
        )
        .await
        .is_err());
        assert!(net::send_report_request(
            &report_client,
            "POST",
            &allowed.origin,
            &BTreeMap::new(),
            vec![],
            true
        )
        .await
        .is_ok());
        let (handle, mut events) = websocket::open(WsRequest {
            url: denied.origin.replace("http:", "ws:"),
            ..Default::default()
        });
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap(),
            Some(WsEvent::Failed)
        );
        drop(handle);
        let (handle, mut events) = websocket::open(WsRequest {
            url: allowed.origin.replace("http:", "ws:"),
            ..Default::default()
        });
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap(),
            Some(WsEvent::Open(_))
        ));
        drop(handle);
        // Process policy is fixed on first native use, not mutable guest/process state.
        std::env::remove_var("TURBO_SURF_ALLOWED_ORIGINS");
        assert!(net::fetch_html(&denied.origin, FetchOptions::default())
            .await
            .is_err());
    }
    if enabled {
        assert_eq!(
            denied.accepts.load(Ordering::SeqCst),
            0,
            "denied peer received a socket"
        );
    }
}

#[test]
fn native_origin_guard() {
    if let Ok(mode) = std::env::var("SURF_ORIGIN_GUARD_TEST_MODE") {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(probe(&mode));
        return;
    }
    let mut modes = vec!["enabled", "absent", "empty", "malformed", "path"];
    if cfg!(unix) {
        modes.push("nonunicode");
    }
    for mode in modes {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "native_origin_guard", "--nocapture"])
            .env("SURF_ORIGIN_GUARD_TEST_MODE", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mode={mode}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
