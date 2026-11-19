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
        lock.lock().unwrap().started = None;
        changed.notify_one();
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
}
