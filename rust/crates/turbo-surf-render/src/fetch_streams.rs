//! Native request/body lifetimes; no V8 objects or realms are held by transport jobs.
use deno_core::{op2, v8, JsRuntime, OpState};
use deno_error::JsErrorBox;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};
use tokio::sync::{watch, Mutex};
use turbo_surf_core::net::ResponseBody;
#[derive(Default)]
struct Jobs {
    next: u32,
    active: HashMap<u32, Arc<Job>>,
    owners: HashMap<u32, v8::Weak<v8::Context>>,
    bodies: HashMap<u32, v8::Weak<v8::Object>>,
}
pub(super) struct Job {
    pub cancel: watch::Sender<bool>,
    pub policy: crate::runtime::FetchPolicySnapshot,
    upload: std::sync::Mutex<Option<turbo_surf_core::upload_body::UploadBody>>,
    upload_sender: std::sync::Mutex<Option<turbo_surf_core::upload_body::UploadSender>>,
    graceful_upload_abort: std::sync::atomic::AtomicBool,
    body: Mutex<Option<ResponseBody>>,
    log_bytes: Arc<std::sync::Mutex<Vec<u8>>>,
    background: std::sync::atomic::AtomicBool,
    log_finished: std::sync::atomic::AtomicBool,
    collected: std::sync::atomic::AtomicBool,
}
pub(super) fn job(state: &OpState, id: u32) -> Option<Arc<Job>> {
    state.try_borrow::<Jobs>()?.active.get(&id).cloned()
}
pub(super) async fn install(job: &Job, body: ResponseBody) -> bool {
    if *job.cancel.borrow() {
        return false;
    }
    let mut slot = job.body.lock().await;
    if *job.cancel.borrow() {
        return false;
    }
    let log = job.log_bytes.clone();
    let observer = Arc::new(move |bytes: &[u8]| {
        let mut log = log.lock().unwrap();
        let remaining = (16 * 1024 * 1024_usize).saturating_sub(log.len());
        log.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
    });
    let body = body.background_cache_with_observer(job.cancel.subscribe(), Some(observer));
    job.background.store(
        body.has_background_cache(),
        std::sync::atomic::Ordering::Release,
    );
    *slot = Some(body);
    true
}
#[op2(fast)]
pub(super) fn op_fetch_prepare(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
) -> Result<u32, JsErrorBox> {
    if !crate::browser_env::is_light_document_node(scope, document) {
        return Err(JsErrorBox::type_error("Fetch document unavailable"));
    }
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Fetch realm unavailable"))?;
    let policy = crate::runtime::snapshot_fetch_policy(context);
    if !state.has::<Jobs>() {
        state.put(Jobs::default())
    }
    sweep_state(scope, state);
    let jobs = state.borrow_mut::<Jobs>();
    if jobs.active.len() >= 256 {
        return Err(JsErrorBox::range_error("Too many active fetch bodies"));
    }
    jobs.next = jobs
        .next
        .checked_add(1)
        .ok_or_else(|| JsErrorBox::range_error("Too many fetch requests"))?;
    let id = jobs.next;
    let (cancel, _) = watch::channel(false);
    jobs.owners.insert(id, v8::Weak::new(scope, context));
    jobs.active.insert(
        id,
        Arc::new(Job {
            cancel,
            policy,
            upload: std::sync::Mutex::new(None),
            upload_sender: std::sync::Mutex::new(None),
            graceful_upload_abort: std::sync::atomic::AtomicBool::new(false),
            body: Mutex::new(None),
            log_bytes: Arc::new(std::sync::Mutex::new(Vec::new())),
            background: std::sync::atomic::AtomicBool::new(false),
            log_finished: std::sync::atomic::AtomicBool::new(false),
            collected: std::sync::atomic::AtomicBool::new(false),
        }),
    );
    Ok(id)
}
pub(super) fn take_upload(job: &Job) -> Option<turbo_surf_core::upload_body::UploadBody> {
    job.upload.lock().unwrap().take()
}
#[op2(fast)]
pub(super) fn op_fetch_upload_start(state: &mut OpState, id: u32) -> Result<(), JsErrorBox> {
    let job = job(state, id).ok_or_else(|| JsErrorBox::type_error("Fetch unavailable"))?;
    let mut slot = job.upload_sender.lock().unwrap();
    if slot.is_some() {
        return Err(JsErrorBox::type_error("Upload already started"));
    }
    let (sender, body) =
        turbo_surf_core::upload::upload_channel(turbo_surf_core::upload::MAX_UPLOAD_BYTES);
    *job.upload.lock().unwrap() = Some(body);
    *slot = Some(sender);
    Ok(())
}
#[op2]
pub(super) async fn op_fetch_upload_write(
    state: Rc<RefCell<OpState>>,
    id: u32,
    #[buffer(copy)] bytes: Vec<u8>,
) -> Result<(), JsErrorBox> {
    if bytes.len() > 64 * 1024 {
        return Err(JsErrorBox::range_error("Upload chunk exceeds native bound"));
    }
    let job = job(&state.borrow(), id).ok_or_else(|| JsErrorBox::type_error("Upload closed"))?;
    let sender = job
        .upload_sender
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| JsErrorBox::type_error("Upload closed"))?;
    let mut cancel = job.cancel.subscribe();
    if *cancel.borrow() {
        return Err(JsErrorBox::type_error("Upload cancelled"));
    }
    tokio::select! {biased;
        _=cancel.changed()=>Err(JsErrorBox::type_error("Upload cancelled")),
        result=sender.write(bytes)=>result.map_err(|error| JsErrorBox::type_error(error.to_string())),
    }
}
#[op2(fast)]
pub(super) fn op_fetch_upload_finish(state: &mut OpState, id: u32) {
    if let Some(job) = job(state, id) {
        if let Some(sender) = job.upload_sender.lock().unwrap().take() {
            sender.finish();
        }
    }
}
/// A user abort finishes an active H2 upload without converting its EOF into a
/// reset. The guest rejection is independent of this bounded native cleanup.
#[op2(fast)]
pub(super) fn op_fetch_abort(state: &mut OpState, id: u32) {
    let Some(jobs) = state.try_borrow_mut::<Jobs>() else {
        return;
    };
    let Some(job) = jobs.active.remove(&id) else {
        return;
    };
    jobs.owners.remove(&id);
    jobs.bodies.remove(&id);
    let sender = job.upload_sender.lock().unwrap().take();
    if let Some(sender) = sender {
        job.graceful_upload_abort
            .store(true, std::sync::atomic::Ordering::Release);
        sender.finish();
        job.cancel.send_replace(true);
    } else {
        cancel(&job);
    }
}

pub(super) async fn until_cancelled<T>(
    job: &Job,
    request: impl std::future::Future<Output = T>,
) -> Option<T> {
    let mut request = std::pin::pin!(request);
    let mut cancelled = job.cancel.subscribe();
    if !*cancelled.borrow() {
        tokio::select! {biased;
            _ = cancelled.changed() => {},
            response = &mut request => return Some(response),
        }
    }
    if job
        .graceful_upload_abort
        .load(std::sync::atomic::Ordering::Acquire)
    {
        // Keep the transport alive long enough to flush the already bounded
        // producer queue and EOF. A stalled peer cannot retain the job forever.
        let _ = tokio::time::timeout(std::time::Duration::from_millis(100), request).await;
    }
    None
}

#[op2(fast)]
pub(super) fn op_fetch_cancel(state: &mut OpState, id: u32) {
    let Some(jobs) = state.try_borrow_mut::<Jobs>() else {
        return;
    };
    let Some(job) = jobs.active.remove(&id) else {
        return;
    };
    jobs.owners.remove(&id);
    jobs.bodies.remove(&id);
    cancel(&job);
}
fn cancel(job: &Job) {
    job.cancel.send_replace(true);
    if let Some(sender) = job.upload_sender.lock().unwrap().take() {
        sender.abort(turbo_surf_core::net::HttpError {
            message: "Upload cancelled".into(),
            code: turbo_surf_core::net::ErrorCode::Network,
        });
    }
    job.upload.lock().unwrap().take();
    if let Ok(mut body) = job.body.try_lock() {
        body.take();
    };
}
#[op2(fast)]
pub(super) fn op_fetch_watch_body(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    object: v8::Local<v8::Object>,
    id: u32,
) {
    let Some(job) = job(state, id) else {
        return;
    };
    let job = Arc::downgrade(&job);
    let weak = v8::Weak::with_guaranteed_finalizer(
        scope,
        object,
        Box::new(move || {
            if let Some(job) = job.upgrade() {
                job.collected
                    .store(true, std::sync::atomic::Ordering::Release);
            }
        }),
    );
    state.borrow_mut::<Jobs>().bodies.insert(id, weak);
}
pub(crate) fn sweep(scope: &mut v8::PinScope) {
    let state = JsRuntime::op_state_from(scope);
    sweep_state(scope, &mut state.borrow_mut());
}
fn sweep_state(scope: &mut v8::PinScope, state: &mut OpState) {
    refresh_logs(state);
    let Some(jobs) = state.try_borrow_mut::<Jobs>() else {
        return;
    };
    let ids: Vec<_> = jobs.active.keys().copied().collect();
    for id in ids {
        let job = jobs.active.get(&id).unwrap().clone();
        let retired = jobs
            .owners
            .get(&id)
            .is_none_or(|owner| owner.to_local(scope).is_none());
        let collected = job.collected.load(std::sync::atomic::Ordering::Acquire)
            || jobs
                .bodies
                .get(&id)
                .is_some_and(|body| body.to_local(scope).is_none());
        if retired {
            cancel(&job);
        } else if collected {
            let Ok(mut body) = job.body.try_lock() else {
                continue;
            };
            if let Some(body) = body.as_mut() {
                if body.detach_cache_consumer() && !body.background_complete() {
                    jobs.bodies.remove(&id);
                    continue;
                }
            }
            drop(body);
            cancel(&job);
        } else {
            continue;
        }
        jobs.active.remove(&id);
        jobs.bodies.remove(&id);
        jobs.owners.remove(&id);
    }
}
pub(super) fn retire_context(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) {
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    let Some(jobs) = state.try_borrow::<Jobs>() else {
        return;
    };
    let ids: Vec<_> = jobs
        .owners
        .iter()
        .filter_map(|(id, owner)| (owner.to_local(scope) == Some(context)).then_some(*id))
        .collect();
    for id in ids {
        release_cancel(&mut state, id);
    }
}
fn release_cancel(state: &mut OpState, id: u32) {
    let Some(jobs) = state.try_borrow_mut::<Jobs>() else {
        return;
    };
    if let Some(job) = jobs.active.remove(&id) {
        cancel(&job);
    }
    jobs.owners.remove(&id);
    jobs.bodies.remove(&id);
}
fn finish_log(state: &mut OpState, id: u32, job: &Job, incomplete: bool) {
    job.log_finished
        .store(true, std::sync::atomic::Ordering::Release);
    let bytes = std::mem::take(&mut *job.log_bytes.lock().unwrap());
    let Some(log) = state.try_borrow_mut::<super::NetworkLog>() else {
        return;
    };
    if let Some(entry) = log.0.iter_mut().find(|entry| entry["streamId"] == id) {
        entry["body"] = String::from_utf8_lossy(&bytes).into_owned().into();
        entry["bodyIncomplete"] = incomplete.into();
    }
}
pub(super) fn refresh_logs(state: &mut OpState) {
    let Some(jobs) = state.try_borrow::<Jobs>() else {
        return;
    };
    let jobs: Vec<_> = jobs
        .active
        .iter()
        .map(|(id, job)| (*id, job.clone()))
        .collect();
    for (id, job) in jobs {
        if !job.background.load(std::sync::atomic::Ordering::Acquire)
            || job.log_finished.load(std::sync::atomic::Ordering::Acquire)
        {
            continue;
        }
        let complete = job
            .body
            .try_lock()
            .ok()
            .and_then(|body| body.as_ref().and_then(ResponseBody::background_status));
        let bytes = job.log_bytes.lock().unwrap().clone();
        let Some(log) = state.try_borrow_mut::<super::NetworkLog>() else {
            continue;
        };
        if let Some(entry) = log.0.iter_mut().find(|entry| entry["streamId"] == id) {
            entry["body"] = String::from_utf8_lossy(&bytes).into_owned().into();
            entry["bodyIncomplete"] = (!complete.unwrap_or(false)).into();
        }
        if complete.is_some() {
            job.log_finished
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}
#[op2(fast)]
pub(super) fn op_fetch_release(state: &mut OpState, id: u32) {
    release(state, id)
}
pub(super) fn release(state: &mut OpState, id: u32) {
    if let Some(jobs) = state.try_borrow_mut::<Jobs>() {
        jobs.active.remove(&id);
        jobs.owners.remove(&id);
        jobs.bodies.remove(&id);
    }
}
#[derive(serde::Serialize)]
pub(super) struct Chunk {
    done: bool,
    bytes: Vec<u8>,
    aborted: bool,
    error: Option<String>,
}
impl Chunk {
    fn end() -> Self {
        Self {
            done: true,
            bytes: Vec::new(),
            aborted: false,
            error: None,
        }
    }
}
#[op2]
#[serde]
pub(super) async fn op_fetch_read(state: Rc<RefCell<OpState>>, id: u32) -> Chunk {
    let Some(job) = job(&state.borrow(), id) else {
        return Chunk {
            aborted: true,
            ..Chunk::end()
        };
    };
    let mut cancel = job.cancel.subscribe();
    let mut body = job.body.lock().await;
    let Some(stream) = body.as_mut() else {
        return Chunk::end();
    };
    enum Read {
        Aborted,
        Chunk(Result<Option<Vec<u8>>, turbo_surf_core::net::HttpError>),
    }
    let result = if *cancel.borrow() {
        Read::Aborted
    } else {
        tokio::select! {biased; _=cancel.changed()=>Read::Aborted,chunk=stream.next_chunk()=>Read::Chunk(chunk)}
    };
    match result {
        Read::Aborted => {
            body.take();
            Chunk {
                aborted: true,
                ..Chunk::end()
            }
        }
        Read::Chunk(Ok(Some(bytes))) => {
            if !job.background.load(std::sync::atomic::Ordering::Acquire) {
                let mut log = job.log_bytes.lock().unwrap();
                let remaining = (16 * 1024 * 1024_usize).saturating_sub(log.len());
                log.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
            }
            Chunk {
                done: false,
                bytes,
                aborted: false,
                error: None,
            }
        }
        Read::Chunk(Ok(None)) => {
            body.take();
            finish_log(&mut state.borrow_mut(), id, &job, false);
            release(&mut state.borrow_mut(), id);
            Chunk::end()
        }
        Read::Chunk(Err(error)) => {
            body.take();
            finish_log(&mut state.borrow_mut(), id, &job, true);
            release(&mut state.borrow_mut(), id);
            Chunk {
                error: Some(error.to_string()),
                ..Chunk::end()
            }
        }
    }
}
