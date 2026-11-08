//! Dedicated Worker runtime (turbo-surf#203): a `new Worker(url)` must fetch the
//! script, run it in a fresh V8 context whose global is a DedicatedWorkerGlobalScope
//! (no DOM), and carry structured-cloned messages across the page↔worker boundary.
//!
//! These cases pin the Chrome 156 contract from the issue's "Done when" list. They
//! are `#[ignore]` until the runtime lands — the current renderer only exposes the
//! CSP-aware construct-only shell introduced in PR #202, so the worker script never
//! executes and `postMessage` is a no-op. Un-ignore each case as its capability lands.
//!
//! Expected oracle: Chrome 156 (156.0.8078.4, headless) running the same script on a
//! localhost page.
#![cfg(turbo_surf_native_proxy)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

async fn serve(
    routes: std::collections::HashMap<&'static str, (&'static str, &'static str)>,
) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let routes = std::sync::Arc::new(routes);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.windows(4).any(|p| p == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (mime, body) = routes
                    .get(path.as_str())
                    .copied()
                    .unwrap_or(("text/plain", "missing"));
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    address
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#203: dedicated Worker runtime not implemented"]
async fn classic_worker_script_runs_and_round_trips_a_postmessage() {
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "/page",
        (
            "text/html",
            r#"<body><script>
              const w = new Worker('/worker.js');
              globalThis.__log = [];
              w.onmessage = (e) => { globalThis.__log.push(['from-worker', e.data]); };
              w.onerror = (e) => { globalThis.__log.push(['error', String(e.message || e)]); };
              w.postMessage({ hello: 'page' });
            </script></body>"#,
        ),
    );
    routes.insert(
        "/worker.js",
        (
            "application/javascript",
            r#"
              self.__isWorker = typeof WorkerGlobalScope !== 'undefined'
                && self instanceof DedicatedWorkerGlobalScope
                && typeof document === 'undefined'
                && typeof window === 'undefined';
              self.onmessage = (e) => {
                self.postMessage({ echoed: e.data, scope: self.__isWorker });
              };
            "#,
        ),
    );
    let address = serve(routes).await;
    let base = format!("http://{address}/page");
    let mut page = PageSession::open("", &base, "", "", 5000).await.unwrap();
    let result = page
        .eval_json_expression(
            r#"(async () => {
                for (let i = 0; i < 50 && globalThis.__log.length === 0; i++) {
                    await new Promise(r => setTimeout(r, 20));
                }
                return globalThis.__log;
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        result,
        serde_json::json!([["from-worker", {"echoed": {"hello": "page"}, "scope": true}]])
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#203: dedicated Worker runtime not implemented"]
async fn worker_global_scope_has_no_dom_but_has_timers_and_fetch() {
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "/page",
        (
            "text/html",
            r#"<body><script>
              const w = new Worker('/worker.js');
              globalThis.__log = null;
              w.onmessage = (e) => { globalThis.__log = e.data; };
            </script></body>"#,
        ),
    );
    routes.insert(
        "/worker.js",
        (
            "application/javascript",
            r#"
              const shape = {
                hasWindow: typeof window !== 'undefined',
                hasDocument: typeof document !== 'undefined',
                hasSelf: typeof self === 'object',
                hasFetch: typeof fetch === 'function',
                hasSetTimeout: typeof setTimeout === 'function',
                hasQueueMicrotask: typeof queueMicrotask === 'function',
                hasImportScripts: typeof importScripts === 'function',
                hasClose: typeof close === 'function',
              };
              setTimeout(() => self.postMessage(shape), 0);
            "#,
        ),
    );
    let address = serve(routes).await;
    let base = format!("http://{address}/page");
    let mut page = PageSession::open("", &base, "", "", 5000).await.unwrap();
    let result = page
        .eval_json_expression(
            r#"(async () => {
                for (let i = 0; i < 50 && globalThis.__log === null; i++) {
                    await new Promise(r => setTimeout(r, 20));
                }
                return globalThis.__log;
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(result["hasWindow"], false);
    assert_eq!(result["hasDocument"], false);
    assert_eq!(result["hasSelf"], true);
    assert_eq!(result["hasFetch"], true);
    assert_eq!(result["hasSetTimeout"], true);
    assert_eq!(result["hasQueueMicrotask"], true);
    assert_eq!(result["hasImportScripts"], true);
    assert_eq!(result["hasClose"], true);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#203: module workers not implemented"]
async fn module_worker_loads_its_import_graph_through_the_net_module_loader() {
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "/page",
        (
            "text/html",
            r#"<body><script>
              const w = new Worker('/worker.mjs', { type: 'module' });
              globalThis.__log = null;
              w.onmessage = (e) => { globalThis.__log = e.data; };
            </script></body>"#,
        ),
    );
    routes.insert(
        "/worker.mjs",
        (
            "application/javascript",
            "import { greet } from './dep.mjs';\nself.postMessage(greet('page'));",
        ),
    );
    routes.insert(
        "/dep.mjs",
        (
            "application/javascript",
            "export const greet = (who) => `hello, ${who}`;",
        ),
    );
    let address = serve(routes).await;
    let base = format!("http://{address}/page");
    let mut page = PageSession::open("", &base, "", "", 5000).await.unwrap();
    let result = page
        .eval_json_expression(
            r#"(async () => {
                for (let i = 0; i < 50 && globalThis.__log === null; i++) {
                    await new Promise(r => setTimeout(r, 20));
                }
                return globalThis.__log;
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(result, serde_json::json!("hello, page"));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#203: terminate()/close() not implemented"]
async fn worker_terminate_stops_further_messages_from_reaching_the_page() {
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "/page",
        (
            "text/html",
            r#"<body><script>
              const w = new Worker('/worker.js');
              globalThis.__log = [];
              w.onmessage = (e) => { globalThis.__log.push(e.data); if (e.data === 'first') w.terminate(); };
              w.postMessage('go');
            </script></body>"#,
        ),
    );
    routes.insert(
        "/worker.js",
        (
            "application/javascript",
            r#"
              self.onmessage = () => {
                self.postMessage('first');
                setTimeout(() => self.postMessage('second'), 20);
              };
            "#,
        ),
    );
    let address = serve(routes).await;
    let base = format!("http://{address}/page");
    let mut page = PageSession::open("", &base, "", "", 5000).await.unwrap();
    let result = page
        .eval_json_expression(
            r#"(async () => {
                await new Promise(r => setTimeout(r, 200));
                return globalThis.__log;
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(result, serde_json::json!(["first"]));
}
