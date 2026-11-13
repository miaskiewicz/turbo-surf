//! Stateful session (G11): a napi `Session` over a retained turbo-dom `Tree` +
//! `CookieJar`, so reads/actions don't reparse and node handles stay stable
//! across calls. The `Tree` is not `Send`, so it lives on a dedicated worker
//! thread (with its own current-thread runtime for `goto`); the napi object
//! holds only a `Send` command channel. One generic `call(tool, argsJson)`
//! dispatches over the session state (mirrors the MCP tool surface).

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde_json::{json, Value};
use std::sync::mpsc::{sync_channel, Sender, SyncSender};
use std::thread;
use turbo_dom_parser::rtdom::serialize::serialize_inner;
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_core::net::{fetch_html, FetchOptions};
use turbo_surf_core::request_context::{ClientHints, RequestContext};
use turbo_surf_view as view;

struct Job {
    tool: String,
    args: Value,
    reply: SyncSender<std::result::Result<String, String>>,
}

#[derive(Default)]
struct State {
    tree: Option<Tree>,
    url: String,
    jar: CookieJar,
    client_hints: ClientHints,
}

/// A retained-DOM browsing session. Methods block on the worker thread.
#[napi]
pub struct Session {
    tx: Sender<Job>,
}

#[napi]
impl Session {
    #[napi(constructor)]
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("session runtime");
            let mut state = State::default();
            for job in rx {
                let res = rt.block_on(dispatch(&mut state, &job.tool, &job.args));
                let _ = job.reply.send(res);
            }
        });
        Self { tx }
    }

    /// Run a tool by name with JSON args → JSON result string. Blocks until the
    /// worker (owning the live `Tree`) replies.
    #[napi]
    pub fn call(&self, tool: String, args_json: Option<String>) -> Result<String> {
        let args: Value = args_json
            .as_deref()
            .map(|s| serde_json::from_str(s).unwrap_or(Value::Null))
            .unwrap_or(Value::Null);
        let (reply, rx) = sync_channel(1);
        self.tx
            .send(Job { tool, args, reply })
            .map_err(|_| Error::from_reason("session worker gone"))?;
        rx.recv()
            .map_err(|_| Error::from_reason("session worker dropped reply"))?
            .map_err(Error::from_reason)
    }
}

fn arg<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str)
}

async fn dispatch(st: &mut State, tool: &str, args: &Value) -> std::result::Result<String, String> {
    match tool {
        // Load HTML directly into the session (no network) — test seam + for
        // callers that fetched elsewhere.
        "load" => {
            st.url = arg(args, "url").unwrap_or("about:blank").to_string();
            st.tree = Some(Tree::parse(arg(args, "html").unwrap_or("")));
            Ok(json!({ "ok": true }).to_string())
        }
        "goto" => goto(st, arg(args, "url").ok_or("goto: missing url")?).await,
        "fill" | "check" | "uncheck" | "select_option" | "click" => action(st, tool, args).await,
        _ => read(st, tool, args),
    }
}

async fn goto(st: &mut State, url: &str) -> std::result::Result<String, String> {
    let opts = FetchOptions {
        allow_non_html: true,
        jar: Some(&mut st.jar),
        client_hints: Some(&st.client_hints),
        context: Some(RequestContext::navigation(
            (!st.url.is_empty()).then(|| st.url.clone()),
            true,
        )),
        ..Default::default()
    };
    let res = fetch_html(url, opts).await.map_err(|e| e.to_string())?;
    st.url = res.final_url.clone();
    st.tree = Some(Tree::parse(&res.html));
    let title = st
        .tree
        .as_ref()
        .unwrap()
        .query_selector("title")
        .map(|h| st.tree.as_ref().unwrap().text_content(h).trim().to_string());
    Ok(
        json!({ "url": res.final_url, "status": res.status, "title": title.unwrap_or_default() })
            .to_string(),
    )
}

fn tree(st: &State) -> std::result::Result<&Tree, String> {
    st.tree
        .as_ref()
        .ok_or_else(|| "no page loaded (call goto)".to_string())
}

async fn action(st: &mut State, tool: &str, args: &Value) -> std::result::Result<String, String> {
    let sel = arg(args, "selector").ok_or("missing selector")?.to_string();
    if tool == "click" {
        let intent = {
            let t = tree(st)?;
            let h = t.query_selector(&sel).ok_or("no match")?;
            view::click_intent(t, h, &st.url)
        };
        return match intent {
            view::ClickIntent::Navigate(u) => goto(st, &u).await,
            view::ClickIntent::Submit(s) => {
                let opts = FetchOptions {
                    method: (s.method != "GET").then_some(s.method),
                    body: s.body,
                    allow_non_html: true,
                    jar: Some(&mut st.jar),
                    client_hints: Some(&st.client_hints),
                    context: Some(RequestContext::navigation(Some(st.url.clone()), true)),
                    ..Default::default()
                };
                let res = fetch_html(&s.url, opts).await.map_err(|e| e.to_string())?;
                st.url = res.final_url.clone();
                st.tree = Some(Tree::parse(&res.html));
                Ok(json!({ "url": res.final_url, "status": res.status }).to_string())
            }
            view::ClickIntent::Inert => Ok(json!({ "action": "inert" }).to_string()),
        };
    }
    let value = arg(args, "value").unwrap_or("").to_string();
    let t = st.tree.as_mut().ok_or("no page loaded (call goto)")?;
    let h = t.query_selector(&sel).ok_or("no match")?;
    match tool {
        "fill" => view::fill_value(t, h, &value),
        "check" => view::set_checked(t, h, true),
        "uncheck" => view::set_checked(t, h, false),
        "select_option" => {
            view::select_option(t, h, &value);
        }
        _ => unreachable!(),
    }
    Ok(json!({ "ok": true }).to_string())
}

fn read(st: &State, tool: &str, args: &Value) -> std::result::Result<String, String> {
    let t = tree(st)?;
    let root = t.root();
    let j = |v: Value| Ok(v.to_string());
    match tool {
        "markdown" => Ok(view::markdown(t, root, &st.url)),
        "text" => Ok(view::text(t, root)),
        "html" => Ok(serialize_inner(t, root)),
        "title" => Ok(t
            .query_selector("title")
            .map(|h| t.text_content(h).trim().to_string())
            .unwrap_or_default()),
        "links" => j(json!(view::links(t, &st.url))),
        "interactive_elements" => j(json!(view::interactive_elements(t, &st.url, true))),
        "accessibility_tree" => j(json!(view::accessibility_tree(t))),
        "hydration_state" => j(json!(view::extract_hydration_state(t))),
        "detect" => j(json!(view::detect_js_required(t, None, None))),
        "query" => read_query(t, root, args),
        "cookies" => Ok(st.jar.storage_state()),
        _ => Err(format!("unknown tool: {tool}")),
    }
}

fn read_query(t: &Tree, root: Handle, args: &Value) -> std::result::Result<String, String> {
    let selector = arg(args, "selector").ok_or("query: missing selector")?;
    let ty = match arg(args, "type") {
        Some("css") => view::QueryType::Css,
        Some("xpath") => view::QueryType::Xpath,
        _ => view::QueryType::Auto,
    };
    Ok(json!(view::query(t, root, selector, ty)).to_string())
}

// ── Live JS sessions ─────────────────────────────────────────────────────────
// A LIVE page keeps the hydrated app's JS isolate (`PageSession`) ALIVE across calls so
// interactions dispatch real DOM events into the running app and the re-render is
// observable (the no-JS `Session` above reparses a static Tree and can't run handlers).
// The V8 isolate is `!Send`, so each session owns a dedicated thread; the registry
// holds only `Send` command channels keyed by id.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc::UnboundedSender as MpscSender;
use turbo_surf_render::{
    PageSession, PageSessionOptions, WindowTimerMode, DEFAULT_RENDER_BUDGET_MS,
};

enum LiveJob {
    Content(
        String,
        String,
        Option<u64>,
        SyncSender<std::result::Result<String, String>>,
    ),
    Resize(u32, u32, SyncSender<std::result::Result<String, String>>),
    Eval(String, SyncSender<std::result::Result<String, String>>),
    EngineEval(String, SyncSender<std::result::Result<String, String>>),
    EngineCollect(String, SyncSender<std::result::Result<String, String>>),
    EngineRead(String, SyncSender<std::result::Result<String, String>>),
    Expression(
        String,
        bool,
        SyncSender<std::result::Result<String, String>>,
    ),
    ExpressionTracked(String, SyncSender<std::result::Result<String, String>>),
    Serialize(SyncSender<String>),
    Cookies(SyncSender<String>),
    SetCookies(String, SyncSender<()>),
    CleanupNetwork,
    Close(SyncSender<()>),
}

struct LiveConnection {
    tx: MpscSender<LiveJob>,
    network: turbo_surf_render::NetworkEventLog,
}
static LIVE: OnceLock<Mutex<HashMap<u32, LiveConnection>>> = OnceLock::new();
static LIVE_NEXT: AtomicU32 = AtomicU32::new(1);

fn live_registry() -> &'static Mutex<HashMap<u32, LiveConnection>> {
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

static OPENING_NETWORKS: OnceLock<Mutex<HashMap<u32, turbo_surf_render::NetworkEventLog>>> =
    OnceLock::new();
fn opening_networks() -> &'static Mutex<HashMap<u32, turbo_surf_render::NetworkEventLog>> {
    OPENING_NETWORKS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn network_log(id: u32) -> Result<turbo_surf_render::NetworkEventLog> {
    let live = live_registry()
        .lock()
        .map_err(|_| Error::from_reason("session registry poisoned"))?;
    if let Some(connection) = live.get(&id) {
        return Ok(connection.network.clone());
    }
    opening_networks()
        .lock()
        .map_err(|_| Error::from_reason("session registry poisoned"))?
        .get(&id)
        .cloned()
        .ok_or_else(|| Error::from_reason("no such live session"))
}
/// Reserve a host event queue before initial hydration starts.
#[napi]
pub fn live_network_log_create() -> Result<u32> {
    let id = LIVE_NEXT.fetch_add(1, Ordering::Relaxed);
    opening_networks()
        .lock()
        .map_err(|_| Error::from_reason("session registry poisoned"))?
        .insert(id, turbo_surf_render::NetworkEventLog::default());
    Ok(id)
}

fn live_send(id: u32, job: LiveJob) -> std::result::Result<(), String> {
    let reg = live_registry()
        .lock()
        .map_err(|_| "session registry poisoned")?;
    let tx = reg.get(&id).ok_or("no such live session")?;
    tx.tx
        .send(job)
        .map_err(|_| "live session worker gone".to_string())
}

/// Spawn a session thread, hydrate the page on it, and register it. Returns the id.
/// Async (hydration fetches chunks from a possibly same-process server, so Node's loop
/// must stay free).
pub struct LiveOpenTask {
    max_resource_bytes: Option<u32>,
    initial_timeout_ms: Option<u32>,
    window_timer_mode: WindowTimerMode,
    response: crate::document_response::DocumentResponse,
    init_script: String,
    engine_init_script: String,
    html: String,
    base_url: String,
    cookies: String,
    user_agent: String,
    client_hints: ClientHints,
    storage: turbo_surf_render::BrowserStorage,
}

#[napi]
impl Task for LiveOpenTask {
    type Output = u32;
    type JsValue = u32;

    fn compute(&mut self) -> Result<u32> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<LiveJob>();
        let (open_tx, open_rx) = sync_channel::<std::result::Result<(), String>>(1);
        let html = std::mem::take(&mut self.html);
        let base = std::mem::take(&mut self.base_url);
        let cookies = std::mem::take(&mut self.cookies);
        let ua = std::mem::take(&mut self.user_agent);
        let client_hints = self.client_hints.clone();
        let storage = self.storage.clone();
        let response = self.response.clone();
        let reserved = response.host_network_log;
        let id = reserved.unwrap_or_else(|| LIVE_NEXT.fetch_add(1, Ordering::Relaxed));
        let network = if reserved.is_some() {
            opening_networks()
                .lock()
                .map_err(|_| Error::from_reason("session registry poisoned"))?
                .get(&id)
                .cloned()
                .ok_or_else(|| Error::from_reason("Invalid opening network log"))?
        } else {
            turbo_surf_render::NetworkEventLog::default()
        };
        let worker_network = network.clone();
        let init_script = std::mem::take(&mut self.init_script);
        let engine_init_script = std::mem::take(&mut self.engine_init_script);
        let initial_timeout_ms = self.initial_timeout_ms;
        let window_timer_mode = self.window_timer_mode;
        let viewport = response.viewport;
        let max_resource_bytes = self.max_resource_bytes;
        thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = open_tx.send(Err(e.to_string()));
                    return;
                }
            };
            let mut session = match rt.block_on(
                PageSession::open_with_document_response_and_host_services(
                    &html,
                    &base,
                    &cookies,
                    &ua,
                    DEFAULT_RENDER_BUDGET_MS,
                    &init_script,
                    client_hints,
                    &response.headers,
                    storage,
                    response.metadata,
                    None,
                    PageSessionOptions {
                        animation_policy: None,
                        max_resource_bytes: max_resource_bytes.map(|bytes| bytes as usize),
                        initial_hydration_budget_ms: initial_timeout_ms.map(u64::from),
                        viewport,
                        window_timer_mode,
                    },
                    &engine_init_script,
                    Some(worker_network.clone()),
                ),
            ) {
                Ok(s) => {
                    if worker_network.is_closed() {
                        s.close();
                        let _ = open_tx.send(Err("Page session closed".into()));
                        return;
                    }
                    if let Some(timeout) =
                        initial_timeout_ms.filter(|_| s.hydration_diagnostics().budget_exceeded)
                    {
                        s.close();
                        let _ = open_tx.send(Err(format!("navigation timeout after {timeout}ms")));
                        return;
                    }
                    let _ = open_tx.send(Ok(()));
                    s
                }
                Err(e) => {
                    let _ = open_tx.send(Err(e));
                    return;
                }
            };
            let mut storage_work = false;
            loop {
                enum Ready {
                    Job(Option<LiveJob>),
                    Storage,
                }
                let ready = if storage_work {
                    rt.block_on(async {
                        tokio::select! {
                            biased;
                            job = rx.recv() => Ready::Job(job),
                            _ = session.eval("") => Ready::Storage,
                        }
                    })
                } else {
                    let signal = session.storage_event_signal();
                    rt.block_on(async {
                        tokio::select! {
                            biased;
                            job = rx.recv() => Ready::Job(job),
                            () = signal => Ready::Storage,
                        }
                    })
                };
                let job = match ready {
                    Ready::Job(Some(job)) => job,
                    Ready::Job(None) => break,
                    Ready::Storage => {
                        // Drive continuations while IO is pending, but let host
                        // commands interrupt the drain (Close cancels transport).
                        storage_work = !storage_work;
                        continue;
                    }
                };
                // Synchronous guest callbacks may create transports, but a read
                // must not poll the runtime or advance the page event loop.
                let _runtime_context = rt.enter();
                session.flush_network_disposals();
                match job {
                    LiveJob::Content(html, wait_until, timeout, reply) => {
                        let result = rt.block_on(session.set_content(&html, &wait_until, timeout));
                        storage_work |= wait_until == "domcontentloaded" && result.is_ok();
                        let _ = reply.send(result);
                    }
                    LiveJob::CleanupNetwork => {}
                    LiveJob::Resize(width, height, reply) => {
                        let result = rt
                            .block_on(session.set_viewport(width, height))
                            .map(|_| String::new());
                        let _ = reply.send(result);
                    }
                    LiveJob::Eval(script, reply) => {
                        let _ = reply.send(rt.block_on(session.eval(&script)));
                    }
                    LiveJob::EngineEval(script, reply) => {
                        let _ = reply.send(rt.block_on(session.eval_engine(&script)));
                    }
                    LiveJob::EngineCollect(script, reply) => {
                        let _ = reply.send(rt.block_on(session.collect_engine(&script)));
                    }
                    LiveJob::EngineRead(script, reply) => {
                        let _ = reply.send(session.read_engine(&script));
                    }
                    LiveJob::Expression(script, engine, reply) => {
                        let result = if engine {
                            rt.block_on(session.eval_engine_expression(&script))
                        } else {
                            rt.block_on(session.eval_expression(&script))
                        };
                        let _ = reply.send(result);
                    }
                    LiveJob::ExpressionTracked(script, reply) => {
                        let _ = reply.send(rt.block_on(session.eval_expression_tracked(&script)));
                    }
                    LiveJob::Serialize(reply) => {
                        let _ = reply.send(session.serialize_for_driver());
                    }
                    LiveJob::SetCookies(cookies, reply) => {
                        session.set_cookies(&cookies);
                        let _ = reply.send(());
                    }
                    LiveJob::Cookies(reply) => {
                        let _ = reply.send(session.cookies_for_driver());
                    }
                    LiveJob::Close(reply) => {
                        session.close();
                        let _ = reply.send(());
                        break;
                    }
                }
            }
        });
        let opened = open_rx
            .recv()
            .map_err(|_| "live session thread died during open".to_owned())
            .and_then(|value| value);
        if let Err(error) = opened {
            opening_networks().lock().unwrap().remove(&id);
            network.close();
            return Err(Error::from_reason(error));
        }
        let mut live = live_registry()
            .lock()
            .map_err(|_| Error::from_reason("session registry poisoned"))?;
        let mut opening = opening_networks()
            .lock()
            .map_err(|_| Error::from_reason("session registry poisoned"))?;
        if network.is_closed() || reserved.is_some() && !opening.contains_key(&id) {
            let (reply, _) = sync_channel(1);
            let _ = tx.send(LiveJob::Close(reply));
            return Err(Error::from_reason("Page session closed"));
        }
        live.insert(id, LiveConnection { tx, network });
        opening.remove(&id);
        Ok(id)
    }

    fn resolve(&mut self, _env: Env, id: u32) -> Result<u32> {
        Ok(id)
    }
}

/// Open a live page session (hydrate + keep alive). Returns a session id.
#[napi]
#[allow(clippy::too_many_arguments)]
pub fn live_open(
    html: String,
    base_url: String,
    cookies: Option<String>,
    user_agent: Option<String>,
    request_state_id: Option<u32>,
    document_response: Option<String>,
    init_script: Option<String>,
    timeout_ms: Option<u32>,
    max_resource_bytes: Option<u32>,
    engine_init_script: Option<String>,
    real_time_timers: Option<bool>,
) -> Result<AsyncTask<LiveOpenTask>> {
    // Parent the V8 platform on the main thread before the live session's worker thread
    // creates the isolate — see `turbo_surf_render::ensure_platform`.
    crate::ensure_render_init();
    let state = crate::request_state(request_state_id)?;
    Ok(AsyncTask::new(LiveOpenTask {
        max_resource_bytes,
        initial_timeout_ms: timeout_ms.filter(|timeout| *timeout > 0),
        window_timer_mode: if real_time_timers.unwrap_or(false) {
            WindowTimerMode::RealTime
        } else {
            WindowTimerMode::Virtual
        },
        response: crate::document_response::parse(document_response)?,
        init_script: init_script.unwrap_or_default(),
        engine_init_script: engine_init_script.unwrap_or_default(),
        html,
        base_url,
        cookies: cookies.unwrap_or_default(),
        user_agent: user_agent.unwrap_or_default(),
        client_hints: state.client_hints,
        storage: state.storage,
    }))
}

pub struct LiveResizeTask {
    id: u32,
    width: u32,
    height: u32,
}

#[napi]
impl Task for LiveResizeTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<String> {
        let (reply, rx) = sync_channel(1);
        live_send(self.id, LiveJob::Resize(self.width, self.height, reply))
            .map_err(Error::from_reason)?;
        rx.recv()
            .map_err(|_| Error::from_reason("live resize worker dropped reply"))?
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, value: String) -> Result<String> {
        Ok(value)
    }
}

/// Update a live session's private host viewport and deliver rendering steps.
#[napi]
pub fn live_resize(id: u32, width: u32, height: u32) -> AsyncTask<LiveResizeTask> {
    AsyncTask::new(LiveResizeTask { id, width, height })
}

/// Run `script` in the live isolate, drain to quiescence, return `String(__RESULT||'')`.
/// Async — the script may trigger fetches to a same-process server.
pub struct LiveEvalTask {
    id: u32,
    script: String,
    expression: bool,
    tracked: bool,
    engine: bool,
    collect: bool,
}

#[napi]
impl Task for LiveEvalTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<String> {
        let (reply, rx) = sync_channel(1);
        live_send(
            self.id,
            if self.tracked {
                LiveJob::ExpressionTracked(std::mem::take(&mut self.script), reply)
            } else if self.expression {
                LiveJob::Expression(std::mem::take(&mut self.script), self.engine, reply)
            } else if self.collect {
                LiveJob::EngineCollect(std::mem::take(&mut self.script), reply)
            } else if self.engine {
                LiveJob::EngineEval(std::mem::take(&mut self.script), reply)
            } else {
                LiveJob::Eval(std::mem::take(&mut self.script), reply)
            },
        )
        .map_err(Error::from_reason)?;
        rx.recv()
            .map_err(|_| Error::from_reason("live session dropped eval reply"))?
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, out: String) -> Result<String> {
        Ok(out)
    }
}

/// Eval JS in a live session (async).
#[napi]
pub fn live_eval(id: u32, script: String) -> AsyncTask<LiveEvalTask> {
    AsyncTask::new(LiveEvalTask {
        id,
        script,
        expression: false,
        tracked: false,
        collect: false,
        engine: false,
    })
}

/// [`live_eval`] for a driver script that needs the engine namespace: the script runs as
/// a function body whose `__tsNs` parameter is the namespace (turbo-dom#222).
#[napi]
pub fn live_eval_engine(id: u32, script: String, collect: Option<bool>) -> AsyncTask<LiveEvalTask> {
    AsyncTask::new(LiveEvalTask {
        id,
        script,
        expression: false,
        tracked: false,
        collect: collect.unwrap_or(false),
        engine: true,
    })
}

/// Evaluate an expression and await its actual native Promise completion.
#[napi]
pub fn live_eval_expression(id: u32, script: String) -> AsyncTask<LiveEvalTask> {
    AsyncTask::new(LiveEvalTask {
        id,
        script,
        expression: true,
        tracked: false,
        collect: false,
        engine: false,
    })
}

/// Evaluate and return a JSON envelope with the result and host-owned sync invalidation.
#[napi]
pub fn live_eval_expression_tracked(id: u32, script: String) -> AsyncTask<LiveEvalTask> {
    AsyncTask::new(LiveEvalTask {
        id,
        script,
        expression: true,
        tracked: true,
        collect: false,
        engine: false,
    })
}

/// Await a trusted driver expression without exposing its namespace to guest functions.
#[napi]
pub fn live_eval_engine_expression(id: u32, script: String) -> AsyncTask<LiveEvalTask> {
    AsyncTask::new(LiveEvalTask {
        id,
        script,
        expression: true,
        tracked: false,
        collect: false,
        engine: true,
    })
}

pub struct LiveContentTask {
    id: u32,
    html: String,
    wait_until: String,
    timeout_ms: Option<u32>,
}

#[napi]
impl Task for LiveContentTask {
    type Output = String;
    type JsValue = String;
    fn compute(&mut self) -> Result<String> {
        let (reply, rx) = sync_channel(1);
        live_send(
            self.id,
            LiveJob::Content(
                std::mem::take(&mut self.html),
                self.wait_until.clone(),
                self.timeout_ms.map(u64::from),
                reply,
            ),
        )
        .map_err(Error::from_reason)?;
        rx.recv()
            .map_err(|_| Error::from_reason("live content worker dropped reply"))?
            .map_err(Error::from_reason)
    }
    fn resolve(&mut self, _env: Env, value: String) -> Result<String> {
        Ok(value)
    }
}

/// Replace live document contents through the native parser/resource lifecycle.
#[napi]
pub fn live_set_content(
    id: u32,
    html: String,
    wait_until: Option<String>,
    timeout_ms: Option<u32>,
) -> AsyncTask<LiveContentTask> {
    AsyncTask::new(LiveContentTask {
        id,
        html,
        wait_until: wait_until.unwrap_or_else(|| "load".into()),
        timeout_ms,
    })
}

/// Drain transport events without queueing behind an isolate awaiting its body.
#[napi]
pub async fn live_network_log(id: u32) -> Result<String> {
    Ok(network_log(id)?.drain_batch())
}

/// Host decisions share the native transport hub without taking the isolate actor.
#[napi]
pub fn live_network_log_publish_intercepted(id: u32, header: String, body: Buffer) -> Result<()> {
    let header: serde_json::Value = serde_json::from_str(&header)
        .map_err(|_| Error::from_reason("Invalid intercepted response"))?;
    if !header.is_object() {
        return Err(Error::from_reason("Invalid intercepted response"));
    }
    network_log(id)?.publish_intercepted_response(header, &body);
    Ok(())
}

/// Wait on native transport notifications without occupying the isolate actor.
#[napi]
pub async fn live_network_log_wait(id: u32, wake: String) -> Result<String> {
    let wake = wake
        .parse::<u64>()
        .map_err(|_| Error::from_reason("Invalid network wake token"))?;
    let log = network_log(id)?;
    let (wake, closed) = log.wait_for_events(wake).await;
    Ok(serde_json::json!({"wake":wake.to_string(),"closed":closed}).to_string())
}
#[napi]
pub fn live_network_log_wake(id: u32) -> Result<()> {
    network_log(id)?.wake();
    Ok(())
}
/// Discard unobserved console argument IDs at the next safe isolate boundary.
#[napi]
pub fn live_network_log_discard_handles(id: u32, ids: String) -> Result<()> {
    let ids = serde_json::from_str::<Vec<u32>>(&ids)
        .map_err(|_| Error::from_reason("Invalid console argument IDs"))?;
    network_log(id)?.discard_handles(ids);
    let _ = live_send(id, LiveJob::CleanupNetwork);
    Ok(())
}

/// Acknowledge only the event batch already processed by the host driver.
#[napi]
pub fn live_network_log_acknowledge(id: u32, revision: String) -> Result<()> {
    let revision = revision
        .parse::<u64>()
        .map_err(|_| Error::from_reason("Invalid network revision"))?;
    if !network_log(id)?.acknowledge(revision) {
        return Err(Error::from_reason("Invalid network revision"));
    }
    Ok(())
}

/// Return a synchronous driver query without pumping the page event loop.
#[napi]
pub fn live_read_engine(id: u32, script: String) -> Result<String> {
    let (reply, rx) = sync_channel(1);
    live_send(id, LiveJob::EngineRead(script, reply)).map_err(Error::from_reason)?;
    // A preceding asynchronous command may await the caller's localhost server.
    // Bound this wait so a synchronous query cannot deadlock that Node event loop.
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| Error::from_reason("live session is busy or engine read timed out"))?
        .map_err(Error::from_reason)
}

/// A non-pumping engine read whose actor queue wait leaves Node available for IO.
pub struct LiveReadTask {
    id: u32,
    script: String,
}

#[napi]
impl Task for LiveReadTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<String> {
        let (reply, rx) = sync_channel(1);
        live_send(
            self.id,
            LiveJob::EngineRead(std::mem::take(&mut self.script), reply),
        )
        .map_err(Error::from_reason)?;
        rx.recv()
            .map_err(|_| Error::from_reason("live session dropped engine read reply"))?
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, value: String) -> Result<String> {
        Ok(value)
    }
}

/// Read the live realm without pumping it, asynchronously waiting for the actor.
#[napi]
pub fn live_read_engine_async(id: u32, script: String) -> AsyncTask<LiveReadTask> {
    AsyncTask::new(LiveReadTask { id, script })
}

/// Serialize the live DOM to HTML (sync — no network).
#[napi]
pub fn live_serialize(id: u32) -> Result<String> {
    let (reply, rx) = sync_channel(1);
    live_send(id, LiveJob::Serialize(reply)).map_err(Error::from_reason)?;
    rx.recv()
        .map_err(|_| Error::from_reason("live session dropped serialize reply"))
}

/// The live session's cookies (storageState JSON) — carries the session a later
/// navigation needs after an in-page login (sync — no network).
#[napi]
pub fn live_cookies(id: u32) -> Result<String> {
    let (reply, rx) = sync_channel(1);
    live_send(id, LiveJob::Cookies(reply)).map_err(Error::from_reason)?;
    rx.recv()
        .map_err(|_| Error::from_reason("live session dropped cookies reply"))
}

/// Replace the live context cookie jar without reopening its document.
#[napi]
pub fn live_set_cookies(id: u32, cookies: String) -> Result<()> {
    let (reply, rx) = sync_channel(1);
    live_send(id, LiveJob::SetCookies(cookies, reply)).map_err(Error::from_reason)?;
    rx.recv()
        .map_err(|_| Error::from_reason("live session dropped cookie update reply"))
}

/// Close a live session: reset the binding, drop the isolate, unregister (sync).
#[napi]
pub fn live_close(id: u32) -> Result<()> {
    let (tx, opening) = {
        let mut live = live_registry()
            .lock()
            .map_err(|_| Error::from_reason("session registry poisoned"))?;
        let mut opening = opening_networks()
            .lock()
            .map_err(|_| Error::from_reason("session registry poisoned"))?;
        (live.remove(&id), opening.remove(&id))
    };
    if let Some(log) = opening {
        log.close();
    }
    if let Some(connection) = tx {
        connection.network.close();
        let (reply, rx) = sync_channel(1);
        if connection.tx.send(LiveJob::Close(reply)).is_ok() {
            let _ = rx.recv();
        }
    }
    Ok(())
}
