//! One operation's hard-execution watchdog, armed only while its future polls.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::{Duration, Instant};

#[derive(Default)]
struct State {
    done: bool,
    started: Option<Instant>,
    generation: u64,
}

struct Supervisor {
    budget: Duration,
    state: Arc<(Mutex<State>, Condvar)>,
    exceeded: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Supervisor {
    fn start(budget: Duration, mut terminate: impl FnMut() + Send + 'static) -> Self {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let exceeded = Arc::new(AtomicBool::new(false));
        let worker_state = state.clone();
        let worker_exceeded = exceeded.clone();
        let thread = std::thread::spawn(move || {
            let (lock, changed) = &*worker_state;
            let mut state = lock.lock().unwrap();
            while !state.done {
                let Some(started) = state.started else {
                    state = changed.wait(state).unwrap();
                    continue;
                };
                let generation = state.generation;
                let wait = if worker_exceeded.load(Ordering::Relaxed) {
                    Duration::from_millis(2)
                } else {
                    budget.saturating_sub(started.elapsed())
                };
                state = changed.wait_timeout(state, wait).unwrap().0;
                if !state.done
                    && state.generation == generation
                    && state.started.is_some_and(|start| start.elapsed() >= budget)
                {
                    worker_exceeded.store(true, Ordering::Relaxed);
                    // Holding the lock orders this request before disarm returns.
                    // Repeat until the poll returns: nested native code may clear V8.
                    terminate();
                }
            }
        });
        Self {
            budget,
            state,
            exceeded,
            thread: Some(thread),
        }
    }
    fn arm(&self) -> Execution<'_> {
        let (lock, changed) = &*self.state;
        let mut state = lock.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        state.started = Some(Instant::now());
        changed.notify_one();
        Execution(self)
    }
    fn disarm(&self) {
        let (lock, changed) = &*self.state;
        let mut state = lock.lock().unwrap();
        state.started = None;
        state.generation = state.generation.wrapping_add(1);
        changed.notify_one();
    }
    fn during_trusted_setup<T>(
        &self,
        run: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _suspended = self.suspend()?;
        run()
    }
    fn suspend(&self) -> Result<SuspendedExecution<'_>, String> {
        let (lock, changed) = &*self.state;
        let mut state = lock.lock().unwrap();
        if self.exceeded.load(Ordering::Relaxed) {
            return Err("Execution interrupted by the session execution budget".into());
        }
        let started = state
            .started
            .ok_or("Trusted setup requires an active execution poll")?;
        let elapsed = started.elapsed();
        if elapsed >= self.budget {
            return Err("Execution budget exhausted before trusted setup".into());
        }
        let generation = state.generation;
        state.started = None;
        changed.notify_one();
        Ok(SuspendedExecution {
            supervisor: self,
            generation,
            elapsed,
        })
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        let (lock, changed) = &*self.state;
        lock.lock().unwrap().done = true;
        changed.notify_one();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
struct Execution<'a>(&'a Supervisor);
impl Drop for Execution<'_> {
    fn drop(&mut self) {
        self.0.disarm();
    }
}

struct SuspendedExecution<'a> {
    supervisor: &'a Supervisor,
    generation: u64,
    elapsed: Duration,
}
impl Drop for SuspendedExecution<'_> {
    fn drop(&mut self) {
        let (lock, changed) = &*self.supervisor.state;
        let mut state = lock.lock().unwrap();
        if !state.done
            && state.generation == self.generation
            && state.started.is_none()
            && !self.supervisor.exceeded.load(Ordering::Relaxed)
        {
            state.started = Some(Instant::now() - self.elapsed);
            changed.notify_one();
        }
    }
}

pub(super) struct PollExecutionBudget {
    supervisor: Supervisor,
    _active: Arc<AtomicBool>,
    deadline: Instant,
    expired: AtomicBool,
}
impl PollExecutionBudget {
    pub(super) fn new(isolate: &mut v8::Isolate, budget_ms: u64) -> Self {
        let active = Arc::new(AtomicBool::new(false));
        if isolate.get_slot::<ActiveWatchdogs>().is_none() {
            isolate.set_slot(ActiveWatchdogs::default());
        }
        let registry = isolate.get_slot_mut::<ActiveWatchdogs>().unwrap();
        registry.0.retain(|flag| flag.strong_count() > 0);
        registry.0.push(Arc::downgrade(&active));
        let handle = isolate.thread_safe_handle();
        Self {
            supervisor: Supervisor::start(Duration::from_millis(budget_ms), move || {
                handle.terminate_execution();
            }),
            _active: active,
            deadline: Instant::now() + Duration::from_millis(budget_ms),
            expired: AtomicBool::new(false),
        }
    }
    // Caller and aggregate deadlines keep running while this poll pauses.
    pub(super) fn during_trusted_setup<T>(
        &self,
        run: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.supervisor.during_trusted_setup(run)
    }
    pub(super) fn interrupted(&self) -> bool {
        self.supervisor.exceeded.load(Ordering::Relaxed)
    }
    pub(super) fn expired(&self) -> bool {
        self.expired.load(Ordering::Relaxed) || Instant::now() >= self.deadline
    }
    // Error provenance differs from a healthy last poll merely finishing late.
    pub(super) fn stopped_at_boundary(&self) -> bool {
        self.expired.load(Ordering::Relaxed)
    }
    pub(super) async fn run<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let mut future = std::pin::pin!(future);
        std::future::poll_fn(|cx| {
            if self.interrupted() {
                return std::task::Poll::Ready(Err(
                    "Execution interrupted by the session execution budget".into(),
                ));
            }
            if Instant::now() >= self.deadline {
                self.expired.store(true, Ordering::Relaxed);
                return std::task::Poll::Ready(Err("Render drain deadline reached".into()));
            }
            let execution = self.supervisor.arm();
            let result = future.as_mut().poll(cx);
            drop(execution);
            result
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn disarm_waits_for_an_in_flight_termination_request() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut first = true;
        let supervisor = Arc::new(Supervisor::start(Duration::from_millis(1), move || {
            if first {
                first = false;
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
        }));
        let execution = supervisor.arm();
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let owner = supervisor.clone();
        let worker = std::thread::spawn(move || {
            owner.disarm();
            done_tx.send(()).unwrap();
        });
        assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
        release_tx.send(()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        drop(execution);
        assert!(supervisor.exceeded.load(Ordering::Relaxed));
    }

    #[test]
    fn an_async_wait_does_not_retain_an_armed_execution_deadline() {
        let (fired_tx, fired_rx) = mpsc::channel();
        let supervisor = Supervisor::start(Duration::from_millis(10), move || {
            fired_tx.send(()).unwrap();
        });
        drop(supervisor.arm());
        assert!(fired_rx.recv_timeout(Duration::from_millis(30)).is_err());
        assert!(!supervisor.exceeded.load(Ordering::Relaxed));
    }
    #[test]
    fn trusted_setup_preserves_consumed_execution_instead_of_paused_wall_time() {
        let supervisor = Supervisor::start(Duration::from_secs(10), || {});
        let execution = supervisor.arm();
        std::thread::sleep(Duration::from_millis(5));
        let suspended = supervisor.suspend().unwrap();
        let consumed = suspended.elapsed;
        assert!(supervisor.state.0.lock().unwrap().started.is_none());
        std::thread::sleep(Duration::from_millis(10));
        let before = Instant::now();
        drop(suspended);
        let after = Instant::now();
        let resumed = supervisor.state.0.lock().unwrap().started.unwrap();
        assert!(resumed >= before - consumed && resumed <= after - consumed);
        drop(execution);
    }

    #[test]
    fn suspended_setup_does_not_rearm_a_disarmed_poll() {
        let supervisor = Supervisor::start(Duration::from_secs(10), || {});
        let execution = supervisor.arm();
        let suspended = supervisor.suspend().unwrap();
        drop(execution);
        drop(suspended);
        assert!(supervisor.state.0.lock().unwrap().started.is_none());
    }

    #[test]
    fn trusted_setup_restores_execution_on_error_and_unwind() {
        let supervisor = Supervisor::start(Duration::from_secs(10), || {});
        let execution = supervisor.arm();
        let result: Result<(), String> = supervisor.during_trusted_setup(|| Err("ordinary".into()));
        assert_eq!(result.unwrap_err(), "ordinary");
        assert!(supervisor.state.0.lock().unwrap().started.is_some());
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<(), String> =
                supervisor.during_trusted_setup(|| panic!("trusted failure"));
        }));
        assert!(panic.is_err());
        assert!(supervisor.state.0.lock().unwrap().started.is_some());
        drop(execution);
    }

    #[test]
    fn a_late_worker_or_prior_stop_cannot_be_forgiven_by_trusted_setup() {
        let supervisor = Supervisor {
            budget: Duration::ZERO,
            state: Arc::new((
                Mutex::new(State {
                    started: Some(Instant::now()),
                    ..Default::default()
                }),
                Condvar::new(),
            )),
            exceeded: Arc::new(AtomicBool::new(false)),
            thread: None,
        };
        assert!(
            matches!(supervisor.suspend(), Err(error) if error == "Execution budget exhausted before trusted setup")
        );
        assert!(supervisor.state.0.lock().unwrap().started.is_some());
        supervisor.exceeded.store(true, Ordering::Relaxed);
        assert!(
            matches!(supervisor.suspend(), Err(error) if error == "Execution interrupted by the session execution budget")
        );
        assert!(supervisor.exceeded.load(Ordering::Relaxed));
    }

    #[test]
    fn trusted_setup_waits_for_an_in_flight_stop_and_preserves_it() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut first = true;
        let supervisor = Arc::new(Supervisor::start(Duration::from_millis(1), move || {
            if first {
                first = false;
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
        }));
        let execution = supervisor.arm();
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (finished_tx, finished_rx) = mpsc::channel();
        let owner = supervisor.clone();
        let waiter = std::thread::spawn(move || {
            finished_tx
                .send(owner.during_trusted_setup(|| Ok(())))
                .unwrap();
        });
        assert!(finished_rx.recv_timeout(Duration::from_millis(20)).is_err());
        release_tx.send(()).unwrap();
        let error = finished_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap_err();
        assert_eq!(
            error,
            "Execution interrupted by the session execution budget"
        );
        assert!(supervisor.exceeded.load(Ordering::Relaxed));
        waiter.join().unwrap();
        drop(execution);
    }

    #[test]
    fn trusted_setup_does_not_suspend_a_parent_supervisor() {
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let parent = Supervisor::start(Duration::from_millis(1), move || {
            let _ = stopped_tx.send(());
        });
        let child = Supervisor::start(Duration::from_secs(10), || {});
        let parent_execution = parent.arm();
        let child_execution = child.arm();
        child
            .during_trusted_setup(|| {
                stopped_rx.recv_timeout(Duration::from_secs(1)).unwrap();
                assert!(parent.exceeded.load(Ordering::Relaxed));
                assert!(!child.exceeded.load(Ordering::Relaxed));
                Ok(())
            })
            .unwrap();
        drop(child_execution);
        drop(parent_execution);
    }
}

#[cfg(test)]
mod expression_contracts {
    use super::*;
    use crate::DocumentResponseMetadata;

    #[tokio::test(flavor = "current_thread")]
    async fn root_expression_fallback_observes_its_own_deadline_between_healthy_tasks() {
        let mut page = PageSession::open_with_document_response_and_options(
            "<!doctype html><p>Owned expression</p>",
            "https://expression-budget.test/",
            "",
            "",
            100,
            "",
            Default::default(),
            &[],
            BrowserStorage::default(),
            DocumentResponseMetadata::default(),
            None,
            PageSessionOptions {
                window_timer_mode: WindowTimerMode::RealTime,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        page.read_engine("globalThis.active=false;globalThis.entered=0;globalThis.done=new Promise(resolve=>{const task=()=>{entered++;active=true;try{const end=performance.now()+70;while(performance.now()<end){}}finally{active=false}setTimeout(task,0)};setTimeout(task,0)});return true;").unwrap();
        let key = page.rt.activation_key();
        let error = key
            .during(page.eval_expression_active("done", false, true))
            .await
            .unwrap_err();
        assert_eq!(error, "Expression exceeded the session execution budget");
        assert!(
            page.read_engine("return entered;")
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert_eq!(page.read_engine("return active;").unwrap(), "false");
        page.close();
    }

    async fn trusted_native_setup_then_authored_callback(hard: bool) {
        let mut page = PageSession::open(
            "<p>Real owner</p>",
            "https://trusted-setup.test/",
            "",
            "",
            5_000,
        )
        .await
        .unwrap();
        let root = page.auxiliary_root_id().unwrap();
        page.eval_target_script(
            root,
            "globalThis.trustedTurn={entered:false,exited:false,active:false};",
        )
        .await
        .unwrap();
        let work = if hard {
            "while(true){}"
        } else {
            "const end=performance.now()+100;while(performance.now()<end){}"
        };
        let script = format!("trustedTurn.entered=true;trustedTurn.active=true;try{{{work}}}finally{{trustedTurn.active=false;trustedTurn.exited=true;}}");
        let execution = PollExecutionBudget::new(page.rt.v8_isolate(), 200);
        let result = execution
            .run(async {
                execution.during_trusted_setup(|| {
                    std::thread::sleep(Duration::from_millis(250));
                    Ok(())
                })?;
                page.eval_target_script(root, &script).await.map(|_| ())
            })
            .await;
        let interrupted = execution.interrupted();
        drop(execution);
        page.rt.v8_isolate().cancel_terminate_execution();
        auxiliary_navigation::retire_interrupted_frames(&mut page.rt);
        let state: serde_json::Value =
            serde_json::from_str(&page.read_target_engine(root, "return trustedTurn").unwrap())
                .unwrap();
        assert_eq!(
            state["entered"], true,
            "real authored callback must enter: {result:?}; {state}"
        );
        if hard {
            assert!(result.is_err(), "{result:?}");
            assert!(
                interrupted,
                "the same parent hard-execution guard must stop the callback"
            );
        } else {
            if let Err(error) = result {
                assert_eq!(error, "Render drain deadline reached");
            }
            assert!(!interrupted);
            assert_eq!(state["active"], false);
            assert_eq!(state["exited"], true);
        }
        page.close();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn trusted_native_setup_excludes_only_its_own_fixed_work() {
        trusted_native_setup_then_authored_callback(false).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn trusted_native_setup_resumes_the_parent_hard_stop_for_authored_code() {
        trusted_native_setup_then_authored_callback(true).await;
    }
}
