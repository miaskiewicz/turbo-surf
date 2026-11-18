//! Driver deadlines do not interrupt trusted backing installation halfway through.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Guest,
    TrustedSetup,
    SettledTimeout,
}

struct Clock {
    started: Instant,
    budget: Duration,
    phase: Phase,
    expired: bool,
}
impl Clock {
    fn expire(&mut self, now: Instant) -> bool {
        self.expired |= now.saturating_duration_since(self.started) >= self.budget;
        self.expired && self.phase == Phase::Guest
    }
    fn enter_setup(&mut self, now: Instant) -> bool {
        self.expire(now);
        if self.expired {
            return false;
        }
        self.phase = Phase::TrustedSetup;
        true
    }
    fn finish_setup(&mut self, now: Instant) -> bool {
        self.expire(now);
        self.phase = if self.expired {
            Phase::SettledTimeout
        } else {
            Phase::Guest
        };
        !self.expired
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ExecutionGoal {
    #[default]
    Commit,
    Interactive,
    Complete,
}
impl ExecutionGoal {
    pub(super) fn requested(value: Option<&str>) -> Self {
        match value {
            Some("domcontentloaded") => Self::Interactive,
            Some("load" | "networkidle") => Self::Complete,
            _ => Self::Commit,
        }
    }
}

#[derive(Default)]
struct Active {
    generation: u64,
    target: u64,
    setup_owner: Option<(u64, u64)>,
    clock: Option<Arc<Mutex<Clock>>>,
    expiry: Arc<AtomicBool>,
    settled: Arc<AtomicBool>,
    goal: ExecutionGoal,
    frames: Vec<DeferredFrame>,
}
#[derive(Clone)]
struct DeferredFrame {
    context: v8::Global<v8::Context>,
    owner: v8::Global<v8::Context>,
    document: v8::Weak<v8::Object>,
    owner_document: v8::Weak<v8::Object>,
    stamp: FrameStamp,
    native_init: bool,
    finished: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrameStamp {
    document: u64,
    native_id: u64,
    owner_document: u64,
    owner_target: u64,
    generation: u64,
}
fn unfinished_owned_frame(
    captured: FrameStamp,
    current: FrameStamp,
    generation: u64,
    owner_target: u64,
    finished: bool,
    documents_match: bool,
) -> bool {
    !finished
        && documents_match
        && captured == current
        && captured.generation == generation
        && captured.owner_target == owner_target
}

fn owns_active(active: &Active, generation: u64, clock: &Arc<Mutex<Clock>>) -> bool {
    active.generation == generation
        && active
            .clock
            .as_ref()
            .is_some_and(|active_clock| Arc::ptr_eq(active_clock, clock))
}

pub(super) struct DriverDeadline {
    shared: Rc<RefCell<OpState>>,
    clock: Arc<Mutex<Clock>>,
    generation: u64,
    target: u64,
    done: Arc<AtomicBool>,
    expiry: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl DriverDeadline {
    pub(super) fn start(
        rt: &mut JsRuntime,
        budget: u64,
        generation: u64,
        target: u64,
        goal: ExecutionGoal,
    ) -> Self {
        let clock = Arc::new(Mutex::new(Clock {
            started: Instant::now(),
            budget: Duration::from_millis(budget),
            phase: Phase::Guest,
            expired: false,
        }));
        let shared = rt.op_state();
        let expiry = Arc::new(AtomicBool::new(false));
        shared.borrow_mut().put(Active {
            generation,
            target,
            setup_owner: None,
            clock: Some(clock.clone()),
            expiry: expiry.clone(),
            settled: Arc::new(AtomicBool::new(false)),
            goal,
            frames: Vec::new(),
        });
        let done = Arc::new(AtomicBool::new(false));
        let worker = spawn(
            rt.v8_isolate().thread_safe_handle(),
            clock.clone(),
            done.clone(),
        );
        Self {
            shared,
            clock,
            generation,
            target,
            done,
            expiry,
            worker: Some(worker),
        }
    }
    pub(super) fn unfinished_frames(&self) -> ExpiredFrames {
        let state = self.shared.borrow();
        let active = state.borrow::<Active>();
        let owned =
            active.target == self.target && owns_active(active, self.generation, &self.clock);
        let frames = if owned {
            active
                .frames
                .iter()
                .filter(|frame| {
                    !frame.finished
                        && frame.stamp.generation == self.generation
                        && frame.stamp.owner_target == self.target
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        ExpiredFrames {
            generation: self.generation,
            target: self.target,
            frames,
        }
    }
    pub(super) fn exceeded(&self) -> bool {
        let mut clock = self.clock.lock().unwrap_or_else(|error| error.into_inner());
        clock.expire(Instant::now());
        if clock.expired {
            self.expiry.store(true, Ordering::Release);
        }
        clock.expired
    }
}
impl Drop for DriverDeadline {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
        let mut shared = self.shared.borrow_mut();
        let active = shared.borrow_mut::<Active>();
        if owns_active(active, self.generation, &self.clock) {
            active.clock = None;
            active.frames.clear();
        }
    }
}
fn spawn(
    handle: v8::IsolateHandle,
    clock: Arc<Mutex<Clock>>,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !done.load(Ordering::Acquire) {
            let mut clock = clock.lock().unwrap_or_else(|error| error.into_inner());
            if clock.expire(Instant::now()) {
                handle.terminate_execution();
            }
            drop(clock);
            std::thread::park_timeout(Duration::from_millis(2));
        }
    })
}

pub(super) struct TrustedSetup(Option<Arc<Mutex<Clock>>>);
impl TrustedSetup {
    pub(super) fn enter(rt: &mut JsRuntime, target: u64, generation: u64) -> Result<Self, String> {
        if rt.v8_isolate().get_microtasks_policy() != v8::MicrotasksPolicy::Explicit {
            return Err("Trusted navigation setup requires explicit microtasks".into());
        }
        let shared = rt.op_state();
        let clock = shared
            .borrow()
            .try_borrow::<Active>()
            .and_then(|active| active.clock.clone());
        if let Some(clock) = &clock {
            if !clock
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .enter_setup(Instant::now())
            {
                return Err("Native target navigation exceeded session budget".into());
            }
        }
        if clock.is_some() {
            shared.borrow_mut().borrow_mut::<Active>().setup_owner = Some((target, generation));
        }
        Ok(Self(clock))
    }
    pub(super) fn finish(mut self) -> Result<(), String> {
        let timely = self.0.take().is_none_or(|clock| {
            clock
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .finish_setup(Instant::now())
        });
        if timely {
            Ok(())
        } else {
            Err("Native target navigation exceeded session budget".into())
        }
    }
}
impl Drop for TrustedSetup {
    fn drop(&mut self) {
        if let Some(clock) = self.0.take() {
            clock
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .finish_setup(Instant::now());
        }
    }
}
pub(super) fn expired(rt: &JsRuntime) -> bool {
    let shared = rt.op_state();
    let clock = shared
        .borrow()
        .try_borrow::<Active>()
        .and_then(|active| active.clock.clone());
    clock.is_some_and(|clock| {
        let mut clock = clock.lock().unwrap_or_else(|error| error.into_inner());
        clock.expire(Instant::now());
        clock.expired
    })
}

pub(super) fn defer_frame_init(
    scope: &mut v8::PinScope,
    native_init: bool,
) -> Result<bool, String> {
    let shared = JsRuntime::op_state_from(scope);
    let clock = shared
        .borrow()
        .try_borrow::<Active>()
        .and_then(|active| active.clock.clone());
    let Some(clock) = clock else {
        return Ok(false);
    };
    let trusted = clock
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .phase
        == Phase::TrustedSetup;
    let context = scope.get_current_context();
    let setup_owner = shared.borrow().borrow::<Active>().setup_owner;
    let Some((owner_target, generation)) = frame_source(scope, context, trusted, setup_owner)
    else {
        return if trusted {
            Err("Deferred initialization source lease unavailable".into())
        } else {
            Ok(false)
        };
    };
    let captured = current_frame_stamp(scope, context, generation)?;
    if captured.1.owner_target != owner_target {
        return if trusted {
            Err("Deferred initialization belongs to another Page".into())
        } else {
            Ok(false)
        };
    }
    let (owner, stamp) = captured;
    let mut captured = DeferredFrame {
        context: v8::Global::new(scope, context),
        owner: v8::Global::new(scope, owner),
        document: context
            .get_slot::<ActiveDocument>()
            .ok_or("Deferred frame document unavailable")?
            .0
            .clone(),
        owner_document: owner
            .get_slot::<ActiveDocument>()
            .ok_or("Deferred frame owner document unavailable")?
            .0
            .clone(),
        stamp,
        native_init,
        finished: false,
    };
    let mut state = shared.borrow_mut();
    let active = state.borrow_mut::<Active>();
    if let Some(frame) = active
        .frames
        .iter_mut()
        .find(|frame| v8::Local::new(scope, &frame.context) == context)
    {
        captured.native_init |= frame.native_init;
        // Each attempt authenticates its actual current Document, including
        // a replacement which starts after trusted setup has finished.
        *frame = captured;
    } else {
        active.frames.push(captured);
    }
    Ok(trusted)
}

#[derive(Clone)]
struct FrameAttempt {
    superseded: bool,
    complete: bool,
    clock: Option<Arc<Mutex<Clock>>>,
    settled: Option<Arc<AtomicBool>>,
    token: v8::Weak<v8::Object>,
    host: v8::Weak<v8::Object>,
    owner: v8::Weak<v8::Context>,
    document: v8::Weak<v8::Object>,
    root: v8::Weak<v8::Context>,
    root_document: v8::Weak<v8::Object>,
    expiry: Option<Arc<AtomicBool>>,
}
const MAX_FRAME_LOAD_ATTEMPTS: usize = 4096;
#[derive(Default)]
struct FrameAttempts(Vec<FrameAttempt>);

fn check_attempt_host<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
) -> Result<(), String> {
    if binding::handle_of(scope, host).is_none() {
        return Err("Frame attempt requires a native host".into());
    }
    let _document = binding::enter_document_of(scope, host);
    if binding::current_document_context(scope) != Some(parent) || native_frame_is_retired(parent) {
        return Err("Frame attempt host document ownership changed".into());
    }
    Ok(())
}

#[derive(Default)]
struct AttemptBudget {
    expiry: Option<Arc<AtomicBool>>,
    clock: Option<Arc<Mutex<Clock>>>,
    settled: Option<Arc<AtomicBool>>,
}

fn attempt_budget<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
) -> AttemptBudget {
    let shared = JsRuntime::op_state_from(scope);
    let active = {
        let state = shared.borrow();
        state.try_borrow::<Active>().and_then(|active| {
            let target = binding::auxiliary_context_identity(root).map(|identity| identity.id.0);
            (active.clock.is_some()
                && active.setup_owner == Some((active.target, active.generation))
                && target == Some(active.target))
            .then(|| AttemptBudget {
                expiry: Some(active.expiry.clone()),
                clock: (active.goal != ExecutionGoal::Commit)
                    .then(|| active.clock.clone())
                    .flatten(),
                settled: Some(active.settled.clone()),
            })
        })
    };
    if let Some(active) = active {
        return active;
    }
    let Some(source) = root.get_slot::<SourceLease>() else {
        return AttemptBudget::default();
    };
    if !same_document(scope, root, &source.document) || source.settled.load(Ordering::Acquire) {
        return AttemptBudget::default();
    }
    let pending = shared.borrow().try_borrow::<State>().is_some_and(|state| {
        state
            .loading
            .get(&source.target)
            .is_some_and(|load| load.generation == source.generation)
    });
    if !pending {
        return AttemptBudget::default();
    }
    let Some(clock) = source.clock.clone() else {
        return AttemptBudget::default();
    };
    let expired = {
        let mut clock = clock.lock().unwrap_or_else(|error| error.into_inner());
        clock.expire(Instant::now());
        clock.expired
    };
    // A fresh explicit intent after the old deadline has an independent command
    // budget. Existing captured attempts retain the expired absolute clock.
    if expired {
        return AttemptBudget::default();
    }
    AttemptBudget {
        expiry: Some(source.expiry.clone()),
        clock: Some(clock),
        settled: Some(source.settled.clone()),
    }
}

pub(super) fn new_frame_attempt<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, String> {
    check_attempt_host(scope, parent, host)?;
    let root = frame_root(scope, parent);
    let document = parent
        .get_slot::<ActiveDocument>()
        .ok_or("Frame attempt document unavailable")?
        .0
        .clone();
    let root_document = root
        .get_slot::<ActiveDocument>()
        .ok_or("Frame attempt root unavailable")?
        .0
        .clone();
    if !same_document(scope, parent, &document) || !same_document(scope, root, &root_document) {
        return Err("Frame attempt document is not current".into());
    }
    let null = v8::null(scope);
    let token = v8::Object::with_prototype_and_properties(scope, null.into(), &[], &[]);
    let budget = attempt_budget(scope, root);
    let attempt = FrameAttempt {
        superseded: false,
        complete: false,
        clock: budget.clock,
        settled: budget.settled,
        token: v8::Weak::new(scope, token),
        host: v8::Weak::new(scope, host),
        owner: v8::Weak::new(scope, parent),
        document,
        root: v8::Weak::new(scope, root),
        root_document,
        expiry: budget.expiry,
    };
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    if !state.has::<FrameAttempts>() {
        state.put(FrameAttempts::default());
    }
    let attempts = state.borrow_mut::<FrameAttempts>();
    attempts.0.retain(|entry| {
        entry.token.to_local(scope).is_some()
            && entry.owner.to_local(scope).is_some()
            && entry.host.to_local(scope).is_some()
    });
    if attempts.0.len() >= MAX_FRAME_LOAD_ATTEMPTS {
        return Err("Frame load attempt limit exceeded".into());
    }
    for entry in &mut attempts.0 {
        if entry
            .host
            .to_local(scope)
            .is_some_and(|saved| saved.strict_equals(host.into()))
        {
            entry.superseded = true;
        }
    }
    attempts.0.push(attempt);
    Ok(token)
}

fn authenticated_attempt<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<FrameAttempt, String> {
    check_attempt_host(scope, parent, host)?;
    let attempt = {
        let shared = JsRuntime::op_state_from(scope);
        let state = shared.borrow();
        state.try_borrow::<FrameAttempts>().and_then(|entries| {
            entries
                .0
                .iter()
                .find(|entry| {
                    entry
                        .token
                        .to_local(scope)
                        .is_some_and(|saved| saved.strict_equals(token.into()))
                })
                .cloned()
        })
    }
    .ok_or("Unknown frame load attempt")?;
    let root = frame_root(scope, parent);
    if attempt.owner.to_local(scope) != Some(parent)
        || attempt.root.to_local(scope) != Some(root)
        || !attempt
            .host
            .to_local(scope)
            .is_some_and(|saved| saved.strict_equals(host.into()))
        || !same_document(scope, parent, &attempt.document)
        || !same_document(scope, root, &attempt.root_document)
    {
        return Err("Frame load attempt ownership changed".into());
    }
    Ok(attempt)
}

fn attempt_expired(attempt: &FrameAttempt) -> bool {
    if attempt.complete {
        return false;
    }
    if attempt
        .expiry
        .as_ref()
        .is_some_and(|expiry| expiry.load(Ordering::Acquire))
    {
        return true;
    }
    if attempt
        .settled
        .as_ref()
        .is_some_and(|settled| settled.load(Ordering::Acquire))
    {
        return false;
    }
    let expired = attempt.clock.as_ref().is_some_and(|clock| {
        let mut clock = clock.lock().unwrap_or_else(|error| error.into_inner());
        clock.expire(Instant::now());
        clock.expired
    });
    if expired {
        if let Some(expiry) = &attempt.expiry {
            expiry.store(true, Ordering::Release);
        }
    }
    expired
}

pub(super) fn frame_attempt_active<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<bool, String> {
    let attempt = authenticated_attempt(scope, parent, host, token)?;
    Ok(!attempt.superseded && !attempt_expired(&attempt))
}

pub(super) fn check_frame_attempt<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<(), String> {
    if frame_attempt_active(scope, parent, host, token)? {
        Ok(())
    } else {
        Err("Frame load attempt is cancelled".into())
    }
}

struct SourceLease {
    modules_cancelled: std::cell::Cell<bool>,
    clock: Option<Arc<Mutex<Clock>>>,
    expiry: Arc<AtomicBool>,
    settled: Arc<AtomicBool>,
    goal: ExecutionGoal,
    target: u64,
    generation: u64,
    document: v8::Weak<v8::Object>,
}

pub(super) fn capture_source_lease<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    target: u64,
    generation: u64,
) -> Result<(), String> {
    let identity = binding::auxiliary_context_identity(context)
        .filter(|identity| !identity.closed)
        .ok_or("Navigation source Page identity unavailable")?;
    if identity.id.0 != target {
        return Err("Navigation source target ownership changed".into());
    }
    let document = context
        .get_slot::<ActiveDocument>()
        .ok_or("Navigation source document unavailable")?
        .0
        .clone();
    if !same_document(scope, context, &document) {
        return Err("Navigation source document ownership changed".into());
    }
    let shared = JsRuntime::op_state_from(scope);
    let (clock, expiry, settled, goal) = {
        let state = shared.borrow();
        match state
            .try_borrow::<Active>()
            .filter(|active| active.target == target && active.generation == generation)
        {
            Some(active) => (
                active
                    .clock
                    .clone()
                    .filter(|_| active.goal != ExecutionGoal::Commit),
                active.expiry.clone(),
                active.settled.clone(),
                active.goal,
            ),
            None => (
                None,
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(true)),
                ExecutionGoal::Commit,
            ),
        }
    };
    context.set_slot(Rc::new(SourceLease {
        modules_cancelled: std::cell::Cell::new(false),
        clock,
        expiry,
        settled,
        goal,
        target,
        generation,
        document,
    }));
    Ok(())
}

pub(super) fn finish_source_goal(
    rt: &mut JsRuntime,
    target: u64,
    generation: u64,
    goal: ExecutionGoal,
) {
    deno_core::scope!(scope, rt);
    let Ok(context) = auxiliary_pages::context(scope, AuxiliaryContextId(target)) else {
        return;
    };
    let Some(source) = context.get_slot::<SourceLease>() else {
        return;
    };
    if source.target == target
        && source.generation == generation
        && source.goal == goal
        && same_document(scope, context, &source.document)
    {
        source.settled.store(true, Ordering::Release);
    }
}

#[derive(Clone)]
struct FrameExecutionSource {
    token: v8::Weak<v8::Object>,
    host: v8::Weak<v8::Object>,
    parent: v8::Weak<v8::Context>,
    document: v8::Weak<v8::Object>,
}

pub(super) fn bind_frame_execution<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<(), String> {
    authenticated_attempt(scope, parent, host, token)?;
    let document = context
        .get_slot::<ActiveDocument>()
        .ok_or("Frame execution document unavailable")?
        .0
        .clone();
    if !same_document(scope, context, &document) {
        return Err("Frame execution document ownership changed".into());
    }
    context.set_slot(Rc::new(FrameExecutionSource {
        token: v8::Weak::new(scope, token),
        host: v8::Weak::new(scope, host),
        parent: v8::Weak::new(scope, parent),
        document,
    }));
    Ok(())
}

pub(super) struct FrameExecutionDeadline {
    clock: Arc<Mutex<Clock>>,
    expiry: Option<Arc<AtomicBool>>,
    done: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for FrameExecutionDeadline {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameExecutionOutcome {
    Completed,
    Cancelled,
    Interrupted,
}
#[derive(Debug)]
pub(super) enum FrameStartError {
    Cancelled,
    Authority(String),
}
impl FrameExecutionDeadline {
    pub(super) fn finish(self, scope: &mut v8::PinScope) -> FrameExecutionOutcome {
        let clock = self.clock.clone();
        let expiry = self.expiry.clone();
        drop(self); // Join before observing the final stop; only the outer owner may clear it.
        let expired = {
            let mut clock = clock.lock().unwrap_or_else(|error| error.into_inner());
            clock.expire(Instant::now());
            clock.expired
        };
        if expired {
            if let Some(expiry) = expiry {
                expiry.store(true, Ordering::Release);
            }
        }
        if scope.is_execution_terminating() {
            FrameExecutionOutcome::Interrupted
        } else if expired {
            FrameExecutionOutcome::Cancelled
        } else {
            FrameExecutionOutcome::Completed
        }
    }
}

pub(super) fn frame_execution_deadline<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<Option<FrameExecutionDeadline>, FrameStartError> {
    let attempt =
        authenticated_attempt(scope, parent, host, token).map_err(FrameStartError::Authority)?;
    if attempt.superseded || attempt_expired(&attempt) {
        return Err(FrameStartError::Cancelled);
    }
    if attempt.complete
        || attempt
            .settled
            .as_ref()
            .is_some_and(|settled| settled.load(Ordering::Acquire))
    {
        return Ok(None);
    }
    let Some(clock) = attempt.clock else {
        return Ok(None);
    };
    let shared = JsRuntime::op_state_from(scope);
    let already_armed = {
        let state = shared.borrow();
        state
            .try_borrow::<Active>()
            .and_then(|active| active.clock.as_ref())
            .is_some_and(|active| Arc::ptr_eq(active, &clock))
    };
    if already_armed {
        return Ok(None);
    }
    let done = Arc::new(AtomicBool::new(false));
    let worker = spawn(scope.thread_safe_handle(), clock.clone(), done.clone());
    Ok(Some(FrameExecutionDeadline {
        clock,
        expiry: attempt.expiry,
        done,
        worker: Some(worker),
    }))
}

pub(super) fn frame_context_deadline<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Result<Option<FrameExecutionDeadline>, FrameStartError> {
    let Some(source) = context.get_slot::<FrameExecutionSource>() else {
        return Ok(None);
    };
    if !same_document(scope, context, &source.document) {
        return Err(FrameStartError::Authority(
            "Frame execution source document changed".into(),
        ));
    }
    let parent = source
        .parent
        .to_local(scope)
        .ok_or_else(|| FrameStartError::Authority("Frame execution owner retired".into()))?;
    let host = source
        .host
        .to_local(scope)
        .ok_or_else(|| FrameStartError::Authority("Frame execution host retired".into()))?;
    let token = source
        .token
        .to_local(scope)
        .ok_or_else(|| FrameStartError::Authority("Frame execution attempt retired".into()))?;
    frame_execution_deadline(scope, parent, host, token)
}

#[derive(Clone)]
struct InterruptedFrame {
    context: v8::Global<v8::Context>,
    document: Option<v8::Weak<v8::Object>>,
    root: v8::Weak<v8::Context>,
    root_document: v8::Weak<v8::Object>,
    previous: Option<v8::Global<v8::Context>>,
}
#[derive(Default)]
struct InterruptedFrames(Vec<InterruptedFrame>);

pub(super) fn queue_interrupted_frame<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    previous: Option<v8::Local<'s, v8::Context>>,
) {
    let root = frame_root(scope, context);
    if root == context {
        return;
    }
    let Some(root_document) = root.get_slot::<ActiveDocument>().map(|doc| doc.0.clone()) else {
        return;
    };
    let frame = InterruptedFrame {
        context: v8::Global::new(scope, context),
        document: context
            .get_slot::<ActiveDocument>()
            .map(|doc| doc.0.clone()),
        root: v8::Weak::new(scope, root),
        root_document,
        previous: previous.map(|value| v8::Global::new(scope, value)),
    };
    context.set_slot(Rc::new(FrameRetired));
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    if !state.has::<InterruptedFrames>() {
        state.put(InterruptedFrames::default());
    }
    let pending = state.borrow_mut::<InterruptedFrames>();
    if !pending
        .0
        .iter()
        .any(|saved| v8::Local::new(scope, &saved.context) == context)
    {
        pending.0.push(frame);
    }
}

pub(super) fn retire_interrupted_frames(rt: &mut OwnedRuntime) {
    if Watchdog::any_active(rt.v8_isolate()) {
        return;
    }
    let shared = rt.op_state();
    let driver_active = shared
        .borrow()
        .try_borrow::<Active>()
        .is_some_and(|active| active.clock.is_some());
    if driver_active || rt.v8_isolate().is_execution_terminating() {
        return;
    }
    let _activation = rt.activation_key().enter();
    settle_deferred_classic_interruptions(rt);
    let frames = {
        let mut state = shared.borrow_mut();
        state
            .try_borrow_mut::<InterruptedFrames>()
            .map(|frames| std::mem::take(&mut frames.0))
            .unwrap_or_default()
    };
    if frames.is_empty() {
        return;
    }
    deno_core::scope!(scope, rt);
    for frame in frames {
        let context = v8::Local::new(scope, &frame.context);
        let Some(root) = frame.root.to_local(scope) else {
            continue;
        };
        if frame_root(scope, context) != root || !same_document(scope, root, &frame.root_document) {
            continue;
        }
        let document_matches = frame.document.as_ref().map_or_else(
            || context.get_slot::<ActiveDocument>().is_none(),
            |doc| same_document(scope, context, doc),
        );
        if !document_matches {
            continue;
        }
        // No guest work can run between this temporary marker removal and the
        // native-only full retirement. This preserves once-only inventory hooks.
        context.remove_slot::<FrameRetired>();
        let previous = frame
            .previous
            .as_ref()
            .map(|previous| v8::Local::new(scope, previous));
        retire_failed_frame_navigation(scope, context, previous);
    }
}

pub(super) fn complete_frame_attempt<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<(), String> {
    let attempt = authenticated_attempt(scope, parent, host, token)?;
    if attempt.superseded || attempt_expired(&attempt) {
        return Err("Frame load attempt is cancelled".into());
    }
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    let entries = state.borrow_mut::<FrameAttempts>();
    if let Some(entry) = entries.0.iter_mut().find(|entry| {
        entry
            .token
            .to_local(scope)
            .is_some_and(|value| value.strict_equals(token.into()))
    }) {
        entry.complete = true;
        entry.clock = None;
    }
    Ok(())
}

fn frame_source<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    trusted: bool,
    setup: Option<(u64, u64)>,
) -> Option<(u64, u64)> {
    if trusted {
        return setup;
    }
    let root = frame_root(scope, context);
    let lease = root.get_slot::<SourceLease>()?;
    same_document(scope, root, &lease.document).then_some((lease.target, lease.generation))
}

fn initialization_is_terminal(terminating: bool, expired: bool) -> bool {
    !terminating && !expired
}

pub(super) fn finish_frame_init(scope: &mut v8::PinScope) {
    let shared = JsRuntime::op_state_from(scope);
    let context = scope.get_current_context();
    let (clock, frame) = {
        let state = shared.borrow();
        let Some(active) = state.try_borrow::<Active>() else {
            return;
        };
        let Some(clock) = active.clock.clone() else {
            return;
        };
        let Some(frame) = active
            .frames
            .iter()
            .find(|frame| v8::Local::new(scope, &frame.context) == context)
            .cloned()
        else {
            return;
        };
        (clock, frame)
    };
    let expired = {
        let mut clock = clock.lock().unwrap_or_else(|error| error.into_inner());
        clock.expire(Instant::now());
        clock.expired
    };
    if !initialization_is_terminal(scope.is_execution_terminating(), expired) {
        return;
    }
    let Ok((owner, stamp)) = current_frame_stamp(scope, context, frame.stamp.generation) else {
        return;
    };
    if frame.stamp != stamp
        || !same_document(scope, context, &frame.document)
        || !same_document(scope, owner, &frame.owner_document)
    {
        return;
    }
    let mut state = shared.borrow_mut();
    let active = state.borrow_mut::<Active>();
    if !active
        .clock
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, &clock))
    {
        return;
    }
    if let Some(saved) = active.frames.iter_mut().find(|saved| saved.stamp == stamp) {
        // Ordinary authored exceptions are terminal too. Only native
        // interruption leaves this exact initialization attempt unfinished.
        saved.finished = true;
    }
}

pub(super) fn run_deferred_frames(rt: &mut JsRuntime) -> Result<(), String> {
    let frames = {
        let shared = rt.op_state();
        let mut shared = shared.borrow_mut();
        shared
            .try_borrow_mut::<Active>()
            .map(|active| {
                active
                    .frames
                    .iter()
                    .filter(|frame| {
                        !frame.finished
                            && active.setup_owner
                                == Some((frame.stamp.owner_target, frame.stamp.generation))
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    deno_core::scope!(scope, rt);
    for frame in frames {
        let context = v8::Local::new(scope, frame.context);
        if native_frame_is_retired(context) {
            continue;
        }
        let scope = &mut v8::ContextScope::new(scope, context);
        if frame.native_init && !binding::run_auxiliary_init_scripts(scope, context) {
            finish_frame_init(scope);
            return Err("Deferred frame init script failed".into());
        }
        run_frame_init_script(scope).map_err(|error| error.to_string())?;
        let shared = JsRuntime::op_state_from(scope);
        let mut state = shared.borrow_mut();
        let active = state.borrow_mut::<Active>();
        if let Some(saved) = active
            .frames
            .iter_mut()
            .find(|saved| saved.stamp == frame.stamp)
        {
            saved.finished = true;
        }
    }
    Ok(())
}

fn frame_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Result<u64, String> {
    let document = context
        .get_slot::<ActiveDocument>()
        .and_then(|document| document.0.to_local(scope))
        .ok_or("Deferred frame document is unavailable")?;
    if document.get_creation_context(scope) != Some(context) {
        return Err("Deferred document context ownership changed".into());
    }
    Ok(binding::document_id_of(scope, document))
}
fn frame_root<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut context: v8::Local<'s, v8::Context>,
) -> v8::Local<'s, v8::Context> {
    while let Some(parent) = context
        .get_slot::<FrameParent>()
        .and_then(|parent| parent.0.to_local(scope))
    {
        context = parent;
    }
    context
}
fn current_frame_stamp<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    generation: u64,
) -> Result<(v8::Local<'s, v8::Context>, FrameStamp), String> {
    let owner = frame_root(scope, context);
    if owner == context
        || context.get_slot::<FrameRetired>().is_some()
        || owner.get_slot::<FrameRetired>().is_some()
        || owner
            .get_slot::<private_boundary::Infrastructure>()
            .is_some()
    {
        return Err("Deferred initialization has no live child-frame owner".into());
    }
    let frame = binding::auxiliary_context_identity(context)
        .filter(|identity| !identity.closed)
        .ok_or("Deferred frame identity unavailable")?;
    let root = binding::auxiliary_context_identity(owner)
        .filter(|identity| !identity.closed)
        .ok_or("Deferred frame Page identity unavailable")?;
    if context.global(scope).get_creation_context(scope) != Some(context)
        || owner.global(scope).get_creation_context(scope) != Some(owner)
        || auxiliary_pages::context(scope, root.id)? != owner
    {
        return Err("Deferred frame ownership has changed".into());
    }
    Ok((
        owner,
        FrameStamp {
            document: frame_document(scope, context)?,
            native_id: frame.id.0,
            owner_document: frame_document(scope, owner)?,
            owner_target: root.id.0,
            generation,
        },
    ))
}
fn same_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    captured: &v8::Weak<v8::Object>,
) -> bool {
    let Some(expected) = captured.to_local(scope) else {
        return false;
    };
    let Some(current) = context
        .get_slot::<ActiveDocument>()
        .and_then(|document| document.0.to_local(scope))
    else {
        return false;
    };
    expected.get_creation_context(scope) == Some(context)
        && current.get_creation_context(scope) == Some(context)
        && expected.strict_equals(current.into())
}

pub(super) struct ExpiredFrames {
    generation: u64,
    target: u64,
    frames: Vec<DeferredFrame>,
}
impl ExpiredFrames {
    pub(super) fn retire(self, rt: &mut JsRuntime) {
        deno_core::scope!(scope, rt);
        for frame in self.frames {
            let context = v8::Local::new(scope, &frame.context);
            let expected_owner = v8::Local::new(scope, &frame.owner);
            let Ok((owner, current)) = current_frame_stamp(scope, context, self.generation) else {
                continue;
            };
            let documents_match = same_document(scope, context, &frame.document)
                && same_document(scope, owner, &frame.owner_document);
            if owner != expected_owner
                || !unfinished_owned_frame(
                    frame.stamp,
                    current,
                    self.generation,
                    self.target,
                    frame.finished,
                    documents_match,
                )
            {
                continue;
            }
            let scope = &mut v8::ContextScope::new(scope, context);
            // Failed-frame retirement is native-only: no partially applied init
            // or parent JavaScript runs while cancelling this exact old attempt.
            retire_failed_frame_tree(scope, context);
        }
    }
}

pub(super) fn reconcile_expired_source_modules(
    rt: &mut OwnedRuntime,
    target: u64,
) -> Result<(), String> {
    let driver_active = rt
        .op_state()
        .borrow()
        .try_borrow::<Active>()
        .is_some_and(|active| active.clock.is_some());
    let cancellation_guarded = driver_active || Watchdog::any_active(rt.v8_isolate());
    let _activation = rt.activation_key().enter();
    deno_core::scope!(scope, rt);
    let context = auxiliary_pages::context(scope, AuxiliaryContextId(target))?;
    let Some(source) = context.get_slot::<SourceLease>() else {
        return Ok(());
    };
    if source.modules_cancelled.get()
        || source.settled.load(Ordering::Acquire)
        || source.target != target
        || !same_document(scope, context, &source.document)
    {
        return Ok(());
    }
    let Some(clock) = &source.clock else {
        return Ok(());
    };
    let expired = {
        let mut clock = clock.lock().unwrap_or_else(|error| error.into_inner());
        clock.expire(Instant::now());
        clock.expired
    };
    if !expired {
        return Ok(());
    }
    let same_generation = JsRuntime::op_state_from(scope)
        .borrow()
        .try_borrow::<State>()
        .is_some_and(|state| {
            state
                .loading
                .get(&target)
                .is_some_and(|load| load.generation == source.generation)
        });
    if !same_generation {
        return Ok(());
    }
    if context.get_slot::<NativePageModuleContext>().is_none() {
        return Ok(());
    }
    if cancellation_guarded || scope.is_execution_terminating() {
        return Err("Module deadline reconciliation requires an inactive command boundary".into());
    }
    if !JsRuntime::cancel_module_context_pending_work(scope, context) {
        return Err("Expired native Page module work could not be cancelled".into());
    }
    source.modules_cancelled.set(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn clock(now: Instant) -> Clock {
        Clock {
            started: now,
            budget: Duration::from_millis(30),
            phase: Phase::Guest,
            expired: false,
        }
    }
    #[test]
    fn large_budgets_do_not_overflow_an_absolute_instant() {
        let now = Instant::now();
        let mut clock = Clock {
            started: now,
            budget: Duration::from_millis(u64::MAX),
            phase: Phase::Guest,
            expired: false,
        };
        assert!(!clock.expire(now + Duration::from_secs(1)));
    }
    #[test]
    fn guest_execution_remains_interruptible_at_original_deadline() {
        let now = Instant::now();
        let mut clock = clock(now);
        assert!(clock.expire(now + Duration::from_millis(30)));
    }
    #[test]
    fn trusted_setup_defers_termination_but_does_not_extend_deadline() {
        let now = Instant::now();
        let mut clock = clock(now);
        assert!(clock.enter_setup(now));
        assert!(!clock.expire(now + Duration::from_millis(31)));
        assert!(!clock.finish_setup(now + Duration::from_millis(32)));
        assert_eq!(clock.phase, Phase::SettledTimeout);
        assert!(clock.expired);
    }
    #[test]
    fn expired_deadline_refuses_setup_before_proxy_rebinding() {
        let now = Instant::now();
        let mut clock = clock(now);
        assert!(!clock.enter_setup(now + Duration::from_millis(31)));
        assert_eq!(clock.phase, Phase::Guest);
    }
    #[test]
    fn timely_setup_returns_guest_execution_to_same_deadline() {
        let now = Instant::now();
        let mut clock = clock(now);
        assert!(clock.enter_setup(now));
        assert!(clock.finish_setup(now + Duration::from_millis(20)));
        assert!(clock.expire(now + Duration::from_millis(30)));
    }
    fn stamp() -> FrameStamp {
        FrameStamp {
            document: 2,
            native_id: 3,
            owner_document: 1,
            owner_target: 1,
            generation: 7,
        }
    }
    #[test]
    fn only_unfinished_same_document_initialization_is_retirable() {
        let captured = stamp();
        assert!(unfinished_owned_frame(
            captured, captured, 7, 1, false, true
        ));
        assert!(!unfinished_owned_frame(
            captured, captured, 7, 1, true, true
        ));
    }
    #[test]
    fn a_new_physical_frame_document_is_not_retired() {
        let captured = stamp();
        assert!(!unfinished_owned_frame(
            captured,
            FrameStamp {
                document: 8,
                ..captured
            },
            7,
            1,
            false,
            true
        ));
        assert!(!unfinished_owned_frame(
            captured,
            FrameStamp {
                native_id: 8,
                ..captured
            },
            7,
            1,
            false,
            true
        ));
    }
    #[test]
    fn a_new_root_document_or_peer_owner_is_not_retired() {
        let captured = stamp();
        assert!(!unfinished_owned_frame(
            captured,
            FrameStamp {
                owner_document: 8,
                ..captured
            },
            7,
            1,
            false,
            true
        ));
        assert!(!unfinished_owned_frame(
            captured,
            FrameStamp {
                owner_target: 8,
                ..captured
            },
            7,
            1,
            false,
            true
        ));
    }
    #[test]
    fn a_new_operation_lease_is_not_retired_or_cleared() {
        let captured = stamp();
        assert!(!unfinished_owned_frame(
            captured, captured, 8, 1, false, true
        ));
        let clock = Arc::new(Mutex::new(clock(Instant::now())));
        let active = Active {
            generation: 8,
            target: 1,
            setup_owner: None,
            clock: Some(clock.clone()),
            expiry: Arc::new(AtomicBool::new(false)),
            settled: Arc::new(AtomicBool::new(false)),
            goal: ExecutionGoal::Complete,
            frames: Vec::new(),
        };
        assert!(!owns_active(&active, 7, &clock));
        assert!(owns_active(&active, 8, &clock));
        let replacement = Arc::new(Mutex::new(super::tests::clock(Instant::now())));
        assert!(!owns_active(&active, 8, &replacement));
    }
    #[test]
    fn numeric_document_ids_alone_never_authorize_retirement() {
        let captured = FrameStamp {
            document: 0,
            owner_document: 0,
            ..stamp()
        };
        assert!(!unfinished_owned_frame(
            captured, captured, 7, 1, false, false
        ));
    }
    #[test]
    fn even_an_authenticated_peer_snapshot_is_not_owned_by_this_driver() {
        let peer = FrameStamp {
            owner_target: 9,
            ..stamp()
        };
        assert!(!unfinished_owned_frame(peer, peer, 7, 1, false, true));
    }
    #[test]
    fn only_native_interruption_keeps_an_initialization_attempt_pending() {
        assert!(initialization_is_terminal(false, false));
        assert!(!initialization_is_terminal(true, false));
        assert!(!initialization_is_terminal(false, true));
    }

    async fn attempt_page() -> PageSession {
        PageSession::open(
            "<body><iframe id=first></iframe><iframe id=second></iframe></body>",
            "https://deadline.test/",
            "",
            "",
            5000,
        )
        .await
        .unwrap()
    }

    fn host(page: &mut PageSession, target: u64, id: &str) -> v8::Global<v8::Value> {
        auxiliary_pages::execute_target(
            &mut page.rt,
            AuxiliaryContextId(target),
            &format!("document.getElementById('{id}')"),
            false,
        )
        .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn frame_attempt_tokens_are_host_bound_superseded_and_terminal() {
        let mut page = attempt_page().await;
        let target = page.replaced_root_target().unwrap();
        let _activation = page.rt.activation_key().enter();
        let first = host(&mut page, target, "first");
        let second = host(&mut page, target, "second");
        deno_core::scope!(scope, &mut page.rt);
        let parent = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
        let scope = &mut v8::ContextScope::new(scope, parent);
        let first = v8::Local::new(scope, first).to_object(scope).unwrap();
        let second = v8::Local::new(scope, second).to_object(scope).unwrap();
        let old = new_frame_attempt(scope, parent, first).unwrap();
        assert!(frame_attempt_active(scope, parent, first, old).unwrap());
        assert_eq!(
            frame_attempt_active(scope, parent, second, old).unwrap_err(),
            "Frame load attempt ownership changed"
        );
        let forged = v8::Object::new(scope);
        assert_eq!(
            check_frame_attempt(scope, parent, first, forged).unwrap_err(),
            "Unknown frame load attempt"
        );
        assert_eq!(
            new_frame_attempt(scope, parent, forged).unwrap_err(),
            "Frame attempt requires a native host"
        );
        let current = new_frame_attempt(scope, parent, first).unwrap();
        assert!(!frame_attempt_active(scope, parent, first, old).unwrap());
        assert_eq!(
            complete_frame_attempt(scope, parent, first, old).unwrap_err(),
            "Frame load attempt is cancelled"
        );
        complete_frame_attempt(scope, parent, first, current).unwrap();
        assert!(frame_attempt_active(scope, parent, first, current).unwrap());
        assert!(frame_execution_deadline(scope, parent, first, current)
            .unwrap()
            .is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn source_goal_completion_requires_the_exact_navigation_lease() {
        let mut page = attempt_page().await;
        let target = page.replaced_root_target().unwrap();
        let _activation = page.rt.activation_key().enter();
        let deadline =
            DriverDeadline::start(&mut page.rt, 5000, 71, target, ExecutionGoal::Complete);
        {
            deno_core::scope!(scope, &mut page.rt);
            let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
            capture_source_lease(scope, root, target, 71).unwrap();
            assert_eq!(
                capture_source_lease(scope, root, target + 1, 71).unwrap_err(),
                "Navigation source target ownership changed"
            );
        }
        finish_source_goal(&mut page.rt, target, 72, ExecutionGoal::Complete);
        finish_source_goal(&mut page.rt, target, 71, ExecutionGoal::Interactive);
        assert!(!deadline
            .shared
            .borrow()
            .borrow::<Active>()
            .settled
            .load(Ordering::Acquire));
        finish_source_goal(&mut page.rt, target, 71, ExecutionGoal::Complete);
        assert!(deadline
            .shared
            .borrow()
            .borrow::<Active>()
            .settled
            .load(Ordering::Acquire));
        assert!(!expired(&page.rt));
        drop(deadline);
        assert!(!expired(&page.rt));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn frame_setup_budget_distinguishes_commit_from_load_and_foreign_generation() {
        let mut page = attempt_page().await;
        let target = page.replaced_root_target().unwrap();
        let _activation = page.rt.activation_key().enter();
        for goal in [ExecutionGoal::Commit, ExecutionGoal::Complete] {
            let deadline = DriverDeadline::start(&mut page.rt, 5000, 81, target, goal);
            let setup = TrustedSetup::enter(&mut page.rt, target, 81).unwrap();
            {
                deno_core::scope!(scope, &mut page.rt);
                let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
                let budget = attempt_budget(scope, root);
                assert!(budget.expiry.is_some());
                assert_eq!(budget.clock.is_some(), goal == ExecutionGoal::Complete);
                assert!(Arc::ptr_eq(
                    budget.settled.as_ref().unwrap(),
                    &deadline.shared.borrow().borrow::<Active>().settled
                ));
            }
            setup.finish().unwrap();
            drop(deadline);
        }
        let deadline =
            DriverDeadline::start(&mut page.rt, 5000, 91, target, ExecutionGoal::Complete);
        let setup = TrustedSetup::enter(&mut page.rt, target, 92).unwrap();
        {
            deno_core::scope!(scope, &mut page.rt);
            let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
            let budget = attempt_budget(scope, root);
            assert!(budget.clock.is_none());
            assert!(budget.expiry.is_none());
        }
        setup.finish().unwrap();
        drop(deadline);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deferred_initialization_tracks_the_actual_child_once_and_finishes_it() {
        let mut page = attempt_page().await;
        let target = page.replaced_root_target().unwrap();
        let _activation = page.rt.activation_key().enter();
        let child_document = auxiliary_pages::execute_target(
            &mut page.rt,
            AuxiliaryContextId(target),
            "document.getElementById('first').contentDocument",
            false,
        )
        .unwrap();
        let deadline =
            DriverDeadline::start(&mut page.rt, 5000, 101, target, ExecutionGoal::Complete);
        let setup = TrustedSetup::enter(&mut page.rt, target, 101).unwrap();
        {
            deno_core::scope!(scope, &mut page.rt);
            let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
            assert_eq!(
                current_frame_stamp(scope, root, 101).unwrap_err(),
                "Deferred initialization has no live child-frame owner"
            );
            let document = v8::Local::new(scope, child_document)
                .to_object(scope)
                .unwrap();
            let child = document.get_creation_context(scope).unwrap();
            assert_ne!(root, child);
            let scope = &mut v8::ContextScope::new(scope, child);
            assert!(defer_frame_init(scope, true).unwrap());
            assert!(defer_frame_init(scope, false).unwrap());
            let shared = JsRuntime::op_state_from(scope);
            {
                let state = shared.borrow();
                let active = state.borrow::<Active>();
                assert_eq!(active.frames.len(), 1);
                assert!(active.frames[0].native_init);
                assert!(!active.frames[0].finished);
            }
            finish_frame_init(scope);
            assert!(shared.borrow().borrow::<Active>().frames[0].finished);
        }
        assert!(deadline.unfinished_frames().frames.is_empty());
        setup.finish().unwrap();
        drop(deadline);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupted_child_classic_script_retires_only_its_attempt_and_same_host_can_retry() {
        let mut page = attempt_page().await;
        let target = page.replaced_root_target().unwrap();
        let _activation = page.rt.activation_key().enter();
        page.eval_expression("globalThis.deadlineSiblingDocument=document.getElementById('second').contentDocument;deadlineSiblingDocument.body.textContent='sibling-survived';'seeded'")
            .await.unwrap();
        let first = host(&mut page, target, "first");
        let child_document = auxiliary_pages::execute_target(
            &mut page.rt,
            AuxiliaryContextId(target),
            "document.getElementById('first').contentDocument",
            false,
        )
        .unwrap();
        let deadline =
            DriverDeadline::start(&mut page.rt, 1000, 111, target, ExecutionGoal::Complete);
        let setup = TrustedSetup::enter(&mut page.rt, target, 111).unwrap();
        let (child, _token) = {
            deno_core::scope!(scope, &mut page.rt);
            let parent = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
            let scope = &mut v8::ContextScope::new(scope, parent);
            let first = v8::Local::new(scope, first).to_object(scope).unwrap();
            let document = v8::Local::new(scope, child_document)
                .to_object(scope)
                .unwrap();
            let child = document.get_creation_context(scope).unwrap();
            let token = new_frame_attempt(scope, parent, first).unwrap();
            bind_frame_execution(scope, child, parent, first, token).unwrap();
            (v8::Global::new(scope, child), v8::Global::new(scope, token))
        };
        setup.finish().unwrap();
        let clock = deadline.clock.clone();
        drop(deadline);
        assert!(!Watchdog::any_active(page.rt.v8_isolate()));
        assert!(page
            .rt
            .op_state()
            .borrow()
            .borrow::<Active>()
            .clock
            .is_none());
        let source: Vec<u16> = "globalThis.deadlineScriptStarted=true;for (;;) {}"
            .encode_utf16()
            .collect();
        let failure = execute_frame_script(
            &mut page.rt,
            child.clone(),
            "https://deadline.test/endless.js",
            &source,
        )
        .expect_err("native deadline must interrupt the actual child script");
        assert!(clock.lock().unwrap().expired);
        assert!(
            failure.deadline_cancelled || failure.deferred_interruption,
            "captured frame deadline must cancel or defer its interrupted child"
        );
        assert_eq!(failure.interrupted, failure.deferred_interruption);
        assert!(!Watchdog::any_active(page.rt.v8_isolate()));
        assert!(page
            .rt
            .op_state()
            .borrow()
            .borrow::<Active>()
            .clock
            .is_none());
        page.rt.v8_isolate().cancel_terminate_execution();
        {
            deno_core::scope!(scope, &mut page.rt);
            let child = v8::Local::new(scope, &child);
            let scope = &mut v8::ContextScope::new(scope, child);
            let key = v8::String::new(scope, "deadlineScriptStarted").unwrap();
            let descriptor = child
                .global(scope)
                .get_own_property_descriptor(scope, key.into())
                .expect("script must actually start before its deadline")
                .to_object(scope)
                .unwrap();
            let value = v8::String::new(scope, "value").unwrap();
            assert!(descriptor.get(scope, value.into()).unwrap().is_true());
        }
        retire_interrupted_frames(&mut page.rt);
        page.eval_expression(
            "document.getElementById('first').srcdoc='<body>retried';'root-alive'",
        )
        .await
        .unwrap();
        assert_eq!(
            page.eval_expression(
                "document.getElementById('first').contentDocument.body.textContent",
            )
            .await
            .unwrap(),
            "retried"
        );
        assert_eq!(
            page.eval_expression(
                "String(document.getElementById('second').contentDocument===deadlineSiblingDocument)+':'+deadlineSiblingDocument.body.textContent",
            )
            .await
            .unwrap(),
            "true:sibling-survived"
        );
    }

    fn capture_deferred_child(page: &mut PageSession, target: u64) -> DriverDeadline {
        let child_document = auxiliary_pages::execute_target(
            &mut page.rt,
            AuxiliaryContextId(target),
            "document.getElementById('first').contentDocument",
            false,
        )
        .unwrap();
        let deadline =
            DriverDeadline::start(&mut page.rt, 5000, 121, target, ExecutionGoal::Complete);
        let setup = TrustedSetup::enter(&mut page.rt, target, 121).unwrap();
        {
            deno_core::scope!(scope, &mut page.rt);
            let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
            capture_source_lease(scope, root, target, 121).unwrap();
            let document = v8::Local::new(scope, child_document)
                .to_object(scope)
                .unwrap();
            let child = document.get_creation_context(scope).unwrap();
            assert_ne!(root, child);
            let scope = &mut v8::ContextScope::new(scope, child);
            assert!(defer_frame_init(scope, false).unwrap());
        }
        setup.finish().unwrap();
        deadline
    }

    async fn remember_sibling(page: &mut PageSession) {
        assert_eq!(page.eval_expression(
            "globalThis.savedDeadlineSibling=document.getElementById('second').contentDocument;savedDeadlineSibling.body.textContent='intact';'saved'",
        ).await.unwrap(), "saved");
    }

    async fn assert_sibling(page: &mut PageSession) {
        assert_eq!(page.eval_expression(
            "String(document.getElementById('second').contentDocument===savedDeadlineSibling)+':'+savedDeadlineSibling.body.textContent",
        ).await.unwrap(), "true:intact");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unfinished_snapshot_retires_only_its_actual_frame_document() {
        let mut page = attempt_page().await;
        let target = page.replaced_root_target().unwrap();
        let _activation = page.rt.activation_key().enter();
        remember_sibling(&mut page).await;
        let deadline = capture_deferred_child(&mut page, target);
        let frames = deadline.unfinished_frames();
        assert_eq!(frames.frames.len(), 1);
        drop(deadline);
        frames.retire(&mut page.rt);
        assert_eq!(
            page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
            1
        );
        assert_sibling(&mut page).await;
        page.eval_expression("document.getElementById('first').srcdoc='<body>new owner';'retry'")
            .await
            .unwrap();
        assert_eq!(
            page.eval_expression(
                "document.getElementById('first').contentDocument.body.textContent"
            )
            .await
            .unwrap(),
            "new owner"
        );
        assert_sibling(&mut page).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deferred_snapshot_cannot_retire_a_later_same_host_document() {
        let mut page = attempt_page().await;
        let target = page.replaced_root_target().unwrap();
        let _activation = page.rt.activation_key().enter();
        remember_sibling(&mut page).await;
        let deadline = capture_deferred_child(&mut page, target);
        let frames = deadline.unfinished_frames();
        assert_eq!(frames.frames.len(), 1);
        drop(deadline);
        assert_eq!(page.eval_expression("new Promise(resolve=>{const frame=document.getElementById('first');frame.onload=()=>{if(frame.contentDocument.body.textContent!=='replacement')return;frame.onload=null;globalThis.savedReplacement=frame.contentDocument;resolve('replaced')};frame.srcdoc='<body>replacement'})")
            .await.unwrap(), "replaced");
        frames.retire(&mut page.rt);
        assert_eq!(page.eval_expression("String(document.getElementById('first').contentDocument===savedReplacement)+':'+savedReplacement.body.textContent")
            .await.unwrap(), "true:replacement");
        assert_sibling(&mut page).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deferred_child_initialization_runs_under_its_captured_source_lease() {
        let mut page = PageSession::open_with_init_script(
            "<body><iframe id=first></iframe><iframe id=second></iframe></body>",
            "https://deadline.test/",
            "",
            "",
            5000,
            "globalThis.deadlineInitCount=(globalThis.deadlineInitCount||0)+1",
        )
        .await
        .unwrap();
        let target = page.replaced_root_target().unwrap();
        let _activation = page.rt.activation_key().enter();
        assert_eq!(
            page.eval_expression(
                "String(document.getElementById('first').contentWindow.deadlineInitCount)"
            )
            .await
            .unwrap(),
            "1"
        );
        let deadline = capture_deferred_child(&mut page, target);
        assert_eq!(deadline.unfinished_frames().frames.len(), 1);
        run_deferred_frames(&mut page.rt).unwrap();
        assert!(deadline.unfinished_frames().frames.is_empty());
        drop(deadline);
        assert_eq!(page.eval_expression("String(document.getElementById('first').contentWindow.deadlineInitCount)+':'+document.getElementById('second').contentWindow.deadlineInitCount")
            .await.unwrap(), "2:1");
    }
}

#[cfg(test)]
#[path = "navigation_deadline_module_tests.rs"]
mod module_tests;

#[cfg(test)]
#[path = "navigation_deadline_coverage_contracts.rs"]
mod coverage_contracts;
