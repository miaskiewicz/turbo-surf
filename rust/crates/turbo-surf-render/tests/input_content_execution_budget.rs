//! Internal drain caps stop between callbacks; explicit caller deadlines remain hard.
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use turbo_surf_render::{PageSession, PageSessionOptions, WindowTimerMode};

const INTERNAL: u64 = 200;
// These cases expire on a controlled pending operation after the healthy turn.
const FINITE_BUDGET: u64 = 1_000;
const HARD: &str = "Execution interrupted by the session execution budget";
fn callback(infinite: bool, finite_ms: u64) -> String {
    let work = if infinite {
        "while(true){}".to_owned()
    } else {
        format!("const until=performance.now()+{finite_ms};while(performance.now()<until){{}}")
    };
    format!("globalThis.turn={{entered:0,active:false,exited:0}};globalThis.runTurn=()=>{{turn.entered++;turn.active=true;try{{{work}}}finally{{turn.active=false;turn.exited++}}}};")
}
async fn open(origin: &str) -> PageSession {
    PageSession::open_with_document_response_and_options(
        "<!doctype html><button id=q>Run</button>",
        origin,
        "",
        "",
        60_000,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}
fn turn(page: &mut PageSession, target: u64) -> Value {
    let raw = page
        .read_target_engine(target, "return globalThis.turn;")
        .unwrap();
    serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("actual selected-state read {raw:?}: {error}"))
}
fn check(page: &mut PageSession, target: u64, result: Result<(), String>, infinite: bool) {
    let state = turn(page, target);
    assert_eq!(
        state["entered"], 1,
        "actual callback must enter: {state}; {result:?}"
    );
    let error = result.unwrap_err();
    if infinite {
        assert!(error.starts_with(HARD), "{error}; {state}");
        assert_eq!(state["exited"], 0);
    } else {
        assert_eq!(
            state["active"], false,
            "finite callback was interrupted: {state}; {error}"
        );
        assert_eq!(state["exited"], 1);
        assert!(
            error.contains("budget") || error.contains("timeout"),
            "{error}"
        );
        assert!(
            !error.starts_with(HARD),
            "aggregate expiry is not a runaway callback: {error}"
        );
    }
}
async fn input(infinite: bool) {
    let mut page = open("http://input-budget.test/").await;
    let target = page.auxiliary_root_id().unwrap();
    page.read_target_engine(
        target,
        &format!(
            "{}document.querySelector('#q').onclick=runTurn;return true;",
            callback(infinite, 25)
        ),
    )
    .unwrap();
    page.set_execution_budget_for_testing(if infinite { INTERNAL } else { FINITE_BUDGET });
    let steps = if infinite {
        json!([{"click":"#q"}])
    } else {
        // Let the finite turn finish before expiring in a separate pending wait.
        json!([{"click":"#q"},{"wait":2 * FINITE_BUDGET}])
    };
    let result = page.input(&steps).await;
    check(&mut page, target, result, infinite);
    page.close();
}
struct ScriptServer {
    origin: String,
    requested: Arc<AtomicBool>,
    release: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl ScriptServer {
    async fn with_held_response() -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let requested = Arc::new(AtomicBool::new(false));
        let seen = requested.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let released = release.clone();
        let body = format!("{}runTurn();", callback(false, 100));
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let body = body.clone();
                let seen = seen.clone();
                let released = released.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    loop {
                        let mut chunk = [0; 1024];
                        let n = socket.read(&mut chunk).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..n]);
                        if request.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    assert!(request.starts_with(b"GET /gate.js "));
                    seen.store(true, Ordering::Release);
                    released.notified().await;
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            origin,
            requested,
            release,
            task,
        }
    }
}
impl Drop for ScriptServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn content_finite(targeted: bool) {
    let server = ScriptServer::with_held_response().await;
    let mut page = open(&server.origin).await;
    let target = page.auxiliary_root_id().unwrap();
    page.set_execution_budget_for_testing(FINITE_BUDGET);
    // The healthy callback completes before the separate held request makes
    // the aggregate content operation expire.
    let html = format!(
        "<!doctype html><p>Replacement</p><script>{}runTurn();</script><script src='/gate.js'></script>",
        callback(false, 25)
    );
    let result = if targeted {
        page.set_target_content(target, &html, "load", None).await
    } else {
        page.set_content(&html, "load", None).await
    };
    assert!(
        server.requested.load(Ordering::Acquire),
        "actual script request must be admitted: {result:?}"
    );
    check(&mut page, target, result.map(|_| ()), false);
    server.release.notify_one();
    page.close();
}

async fn content_runaway(targeted: bool, caller: Option<u64>, internal: u64) {
    let mut page = open("http://runaway-content-budget.test/").await;
    let target = page.auxiliary_root_id().unwrap();
    page.set_execution_budget_for_testing(internal);
    // Run an inline callback with no natural completion so network delivery
    // and runner speed cannot decide whether it outlives the the applicable deadline.
    let html = format!(
        "<!doctype html><p>Replacement</p><script>{}runTurn();</script>",
        callback(true, 0)
    );
    let result = if targeted {
        page.set_target_content(target, &html, "load", caller).await
    } else {
        page.set_content(&html, "load", caller).await
    };
    let state = turn(&mut page, target);
    assert_eq!(
        state["entered"], 1,
        "caller-deadline precondition: {state}; {result:?}"
    );
    assert_eq!(state["exited"], 0);
    let expected = if caller == Some(INTERNAL) {
        "setContent timeout after 200ms"
    } else {
        HARD
    };
    assert_eq!(result.unwrap_err(), expected);
    page.close();
}
// Request admission does not imply that an external script has arrived. Hold
// its real response so the earlier internal deadline has an unambiguous cause.
async fn content_internal_expiry(targeted: bool) {
    let server = ScriptServer::with_held_response().await;
    let mut page = open(&server.origin).await;
    let target = page.auxiliary_root_id().unwrap();
    page.set_execution_budget_for_testing(INTERNAL);
    let html = "<!doctype html><p>Replacement</p><script src='/gate.js'></script>";
    let result = if targeted {
        page.set_target_content(target, html, "load", Some(1000))
            .await
    } else {
        page.set_content(html, "load", Some(1000)).await
    };
    assert!(server.requested.load(Ordering::Acquire));
    assert_eq!(result.unwrap_err(), "setContent timeout after 200ms");
    assert_eq!(
        page.read_target_engine(target, "return typeof globalThis.turn;")
            .unwrap(),
        "\"undefined\""
    );
    server.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            page.poll_background_work().await.unwrap();
            if page
                .read_target_engine(target, "return globalThis.turn?.exited === 1;")
                .unwrap()
                == "true"
            {
                break;
            }
        }
    })
    .await
    .expect("the retained script must finish after its response is released");
    assert_eq!(
        turn(&mut page, target),
        json!({"entered":1,"active":false,"exited":1})
    );
    page.close();
}
#[tokio::test(flavor = "current_thread")]
async fn input_aggregate_expiry_preserves_a_finite_callback_tail() {
    input(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn input_infinite_callback_is_hard_interrupted() {
    input(true).await;
}
#[tokio::test(flavor = "current_thread")]
async fn root_content_without_caller_deadline_preserves_a_finite_callback_tail() {
    content_finite(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn target_content_without_caller_deadline_preserves_a_finite_callback_tail() {
    content_finite(true).await;
}
#[tokio::test(flavor = "current_thread")]
async fn root_content_infinite_callback_is_hard_interrupted() {
    content_runaway(false, None, INTERNAL).await;
}
#[tokio::test(flavor = "current_thread")]
async fn target_content_infinite_callback_is_hard_interrupted() {
    content_runaway(true, None, INTERNAL).await;
}
#[tokio::test(flavor = "current_thread")]
async fn root_content_explicit_caller_deadline_remains_hard() {
    content_runaway(false, Some(INTERNAL), 5_000).await;
}
#[tokio::test(flavor = "current_thread")]
async fn target_content_explicit_caller_deadline_remains_hard() {
    content_runaway(true, Some(INTERNAL), 5_000).await;
}

#[tokio::test(flavor = "current_thread")]
async fn root_content_reports_internal_expiry_before_a_later_caller_deadline() {
    content_internal_expiry(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn target_content_reports_internal_expiry_before_a_later_caller_deadline() {
    content_internal_expiry(true).await;
}
#[tokio::test(flavor = "current_thread")]
async fn root_content_internal_hard_stop_is_not_relabelled_as_a_later_caller_timeout() {
    content_runaway(false, Some(1000), INTERNAL).await;
}
#[tokio::test(flavor = "current_thread")]
async fn target_content_internal_hard_stop_is_not_relabelled_as_a_later_caller_timeout() {
    content_runaway(true, Some(1000), INTERNAL).await;
}
