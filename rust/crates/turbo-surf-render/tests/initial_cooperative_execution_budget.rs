//! Every suspending native entry must stop aggregate work between healthy callbacks.
use serde_json::Value;
use turbo_surf_render::{
    AuxiliaryHostServices, InitialHydrationGoal, PageSession, PageSessionOptions, WindowTimerMode,
};

const BUDGET: u64 = 200;
const HOST: &str = r#"
const bag=__tsNs.shimBindings ||= {__proto__:null,n:0,pending:{__proto__:null}};
globalThis.hostWait=()=>{__tsNs.shimBindingPending(true);return __tsNs.shimBindingPromise(resolve=>{
  __tsNs.finishHost=()=>{__tsNs.shimBindingPending(false);resolve();};
});};
"#;

fn callback(hard: bool) -> String {
    let body = if hard {
        "while(true){}"
    } else {
        "const until=performance.now()+100;while(performance.now()<until){}"
    };
    format!("globalThis.turn={{entered:0,exited:0,active:false}};globalThis.runTurn=()=>{{turn.entered++;turn.active=true;try{{{body}}}finally{{turn.active=false;turn.exited++;}}}};")
}
fn html(hard: bool, host: bool) -> String {
    format!("<!doctype html><body><script type=module>{}{}await fetch('/gate').then(r=>r.text());runTurn();</script>", callback(hard), if host { "await hostWait();" } else { "" })
}
async fn open(html: &str, host: bool, origin: &str) -> PageSession {
    open_custom(html, host, origin, BUDGET, None).await
}
async fn open_custom(
    html: &str,
    host: bool,
    origin: &str,
    budget: u64,
    deadline: Option<std::time::Instant>,
) -> PageSession {
    PageSession::open_with_document_response_and_auxiliary_services(
        html,
        origin,
        "",
        "",
        budget,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            initial_hydration_goal: InitialHydrationGoal::Load,
            initial_hydration_budget_ms: Some(budget),
            initial_hydration_deadline: deadline,
            ..Default::default()
        },
        if host { HOST } else { "" },
        None,
        AuxiliaryHostServices::default(),
    )
    .await
    .unwrap()
}
fn state(page: &mut PageSession, target: Option<u64>) -> Value {
    let value = match target {
        Some(target) => page.read_target_engine(target, "return globalThis.turn;"),
        None => page.read_engine("return globalThis.turn;"),
    }
    .unwrap();
    serde_json::from_str(&value)
        .unwrap_or_else(|error| panic!("actual selected-state read {value:?}: {error}"))
}
fn check(page: &mut PageSession, hard: bool, result: &Result<String, String>, target: Option<u64>) {
    eprintln!("actual entry outcome={result:?}, target={target:?}");
    let value = state(page, target);
    assert_eq!(
        value["entered"], 1,
        "actual callback must enter: {value}; {result:?}"
    );
    if hard {
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .starts_with("Execution interrupted by the session execution budget"),
            "{result:?}; {value}"
        );
        assert_eq!(
            value["exited"], 0,
            "an actual infinite callback cannot complete"
        );
    } else {
        assert_eq!(
            value["active"], false,
            "aggregate expiry interrupted a healthy callback: {value}; {result:?}"
        );
        assert_eq!(
            value["exited"], 1,
            "healthy callback finally must complete: {value}; {result:?}"
        );
        assert!(
            result.is_err(),
            "the original aggregate deadline still applies: {result:?}"
        );
    }
}
struct Fixture {
    origin: String,
    release_at: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    admitted: std::sync::Arc<tokio::sync::Notify>,
    release: std::sync::Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn start(hard: bool) -> Self {
        Self::with_html(html(hard, false)).await
    }
    async fn with_html(body: String) -> Self {
        Self::with_gate(body, false).await
    }
    async fn with_gate(body: String, held: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let release_at = std::sync::Arc::new(std::sync::Mutex::new(None));
        let gate_clock = release_at.clone();
        let admitted = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let requested = admitted.clone();
        let released = release.clone();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            while let Ok((mut socket, _)) = listener.accept().await {
                let document = body.clone();
                let clock = gate_clock.clone();
                let requested = requested.clone();
                let released = released.clone();
                connections.spawn(async move {
                    use tokio::io::{AsyncReadExt,AsyncWriteExt};
                    let mut request=[0;4096];let size=socket.read(&mut request).await.unwrap();
                    let gate=String::from_utf8_lossy(&request[..size]).starts_with("GET /gate ");
                    let (kind,body)=if gate {
                        requested.notify_one();
                        if held { released.notified().await; }
                        else {
                        let release=*clock.lock().unwrap();
                        if let Some(release)=release { tokio::time::sleep_until(tokio::time::Instant::from_std(release)).await; }
                        else { tokio::time::sleep(std::time::Duration::from_millis(140)).await; }
                        }
                        ("text/plain","ready".to_owned())
                    } else {("text/html",document)};
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    let _=socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            origin,
            task,
            release_at,
            admitted,
            release,
        }
    }
    async fn admit(&self, page: &mut PageSession) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let admitted = self.admitted.notified();
            tokio::pin!(admitted);
            loop {
                tokio::select! {
                    biased;
                    () = &mut admitted => break,
                    result = page.poll_background_work() => { result.unwrap(); }
                }
            }
        })
        .await
        .expect("the native gate request must be admitted before the measured continuation");
    }
    fn release_after(&self, delay: std::time::Duration) {
        let release = self.release.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            release.notify_one();
        });
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn contract(continuation: bool, hard: bool) {
    let document = if continuation {
        format!("<!doctype html><body><script type=module>{}const host=hostWait();const response=fetch('/gate');await host;await response.then(r=>r.text());runTurn();</script>", callback(hard))
    } else {
        html(hard, false)
    };
    let fixture = Fixture::with_gate(document.clone(), continuation).await;
    let mut page = open(&document, continuation, &fixture.origin).await;
    let result: Result<String, String> = if continuation {
        fixture.admit(&mut page).await;
        assert_eq!(state(&mut page, None)["entered"], 0);
        page.read_engine("__tsNs.finishHost();return true;")
            .unwrap();
        // Time the held native reply from this entry, after cold request admission.
        fixture.release_after(std::time::Duration::from_millis(140));
        page.continue_initial_hydration()
            .await
            .map(|value| value.as_str().into())
    } else {
        let diagnostic = page.hydration_diagnostics();
        assert!(
            diagnostic.budget_exceeded,
            "aggregate initial budget must expire"
        );
        Err(diagnostic.open_error.clone().unwrap())
    };
    if continuation && !hard {
        assert_eq!(
            result.as_ref().unwrap_err(),
            &format!("navigation timeout after {BUDGET}ms")
        );
    }
    check(&mut page, hard, &result, None);
    page.close();
}
#[tokio::test(flavor = "current_thread")]
async fn initial_open_finishes_a_healthy_callback_before_aggregate_expiry() {
    contract(false, false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn initial_open_reports_a_genuine_hard_interruption() {
    contract(false, true).await;
}
#[tokio::test(flavor = "current_thread")]
async fn initial_continuation_finishes_a_healthy_callback_before_aggregate_expiry() {
    contract(true, false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn initial_continuation_reports_a_genuine_hard_interruption() {
    contract(true, true).await;
}
#[tokio::test(flavor = "current_thread")]
async fn plain_native_fetch_fixture_finishes_without_busy_work() {
    let document="<!doctype html><script type=module>globalThis.turn={entered:0,exited:0,active:false};await fetch('/gate').then(r=>r.text());turn.entered=1;turn.exited=1;</script>";
    let fixture = Fixture::with_html(document.into()).await;
    let mut page = open_custom(document, false, &fixture.origin, 1_000, None).await;
    assert!(!page.hydration_diagnostics().budget_exceeded);
    assert_eq!(state(&mut page, None)["entered"], 1);
    page.close();
}
async fn caller_deadline(continuation: bool) {
    use std::time::{Duration, Instant};
    let fixture = Fixture::start(true).await;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(2);
    *fixture.release_at.lock().unwrap() = Some(deadline - Duration::from_millis(750));
    let mut page = open_custom(
        &html(true, continuation),
        continuation,
        &fixture.origin,
        5000,
        Some(deadline),
    )
    .await;
    let result: Result<String, String> = if continuation {
        tokio::time::sleep(Duration::from_millis(140)).await;
        page.read_engine("__tsNs.finishHost();return true;")
            .unwrap();
        page.continue_initial_hydration()
            .await
            .map(|value| value.as_str().into())
    } else {
        assert!(page.hydration_diagnostics().budget_exceeded);
        Err(page.hydration_diagnostics().open_error.clone().unwrap())
    };
    let error = result.unwrap_err();
    assert!(
        error.starts_with("Execution interrupted by the session execution budget"),
        "{error}"
    );
    let value = state(&mut page, None);
    assert_eq!(
        value["entered"], 1,
        "actual callback must enter before caller deadline"
    );
    assert_eq!(
        value["exited"], 0,
        "explicit caller deadline must interrupt rather than extend to a fresh segment"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    page.close();
}
#[tokio::test(flavor = "current_thread")]
async fn initial_open_preserves_the_explicit_absolute_caller_deadline() {
    caller_deadline(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn initial_host_handoff_does_not_restart_the_explicit_caller_deadline() {
    caller_deadline(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_already_expired_initial_caller_reports_truthful_progress_without_entering_guest_code() {
    use std::time::{Duration, Instant};
    let fixture = Fixture::start(false).await;
    let mut page = open_custom(
        "<!doctype html><body><script>document.body.dataset.guestEntered='yes';</script>",
        false,
        &fixture.origin,
        1000,
        Some(Instant::now() - Duration::from_millis(1)),
    )
    .await;
    let diagnostic = page.hydration_diagnostics();
    assert!(diagnostic.budget_exceeded);
    assert_eq!(
        diagnostic.progress.phase,
        turbo_surf_render::HydrationPhase::DocumentLoading
    );
    assert_eq!(diagnostic.progress.completed_pumps, 0);
    assert!(diagnostic
        .open_error
        .as_ref()
        .unwrap()
        .starts_with("Execution interrupted by the session execution budget"));
    assert!(!page.serialize().contains("data-guest-entered=\"yes\""));
    page.close();
}
