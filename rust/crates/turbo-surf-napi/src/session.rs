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
use std::sync::mpsc::Sender as MpscSender;
use std::sync::{Mutex, OnceLock};
use turbo_surf_render::{PageSession, DEFAULT_RENDER_BUDGET_MS};

enum LiveJob {
    Eval(String, SyncSender<std::result::Result<String, String>>),
    Serialize(SyncSender<String>),
    Cookies(SyncSender<String>),
    Close(SyncSender<()>),
}

static LIVE: OnceLock<Mutex<HashMap<u32, MpscSender<LiveJob>>>> = OnceLock::new();
static LIVE_NEXT: AtomicU32 = AtomicU32::new(1);

fn live_registry() -> &'static Mutex<HashMap<u32, MpscSender<LiveJob>>> {
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn live_send(id: u32, job: LiveJob) -> std::result::Result<(), String> {
    let reg = live_registry()
        .lock()
        .map_err(|_| "session registry poisoned")?;
    let tx = reg.get(&id).ok_or("no such live session")?;
    tx.send(job)
        .map_err(|_| "live session worker gone".to_string())
}

/// Spawn a session thread, hydrate the page on it, and register it. Returns the id.
/// Async (hydration fetches chunks from a possibly same-process server, so Node's loop
/// must stay free).
pub struct LiveOpenTask {
    html: String,
    base_url: String,
    cookies: String,
    user_agent: String,
}

#[napi]
impl Task for LiveOpenTask {
    type Output = u32;
    type JsValue = u32;

    fn compute(&mut self) -> Result<u32> {
        let (tx, rx) = std::sync::mpsc::channel::<LiveJob>();
        let (open_tx, open_rx) = sync_channel::<std::result::Result<(), String>>(1);
        let html = std::mem::take(&mut self.html);
        let base = std::mem::take(&mut self.base_url);
        let cookies = std::mem::take(&mut self.cookies);
        let ua = std::mem::take(&mut self.user_agent);
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
            let mut session = match rt.block_on(PageSession::open(
                &html,
                &base,
                &cookies,
                &ua,
                DEFAULT_RENDER_BUDGET_MS,
            )) {
                Ok(s) => {
                    let _ = open_tx.send(Ok(()));
                    s
                }
                Err(e) => {
                    let _ = open_tx.send(Err(e));
                    return;
                }
            };
            for job in rx {
                match job {
                    LiveJob::Eval(script, reply) => {
                        let _ = reply.send(rt.block_on(session.eval(&script)));
                    }
                    LiveJob::Serialize(reply) => {
                        let _ = reply.send(session.serialize());
                    }
                    LiveJob::Cookies(reply) => {
                        let _ = reply.send(session.cookies());
                    }
                    LiveJob::Close(reply) => {
                        session.close();
                        let _ = reply.send(());
                        break;
                    }
                }
            }
        });
        open_rx
            .recv()
            .map_err(|_| Error::from_reason("live session thread died during open"))?
            .map_err(Error::from_reason)?;
        let id = LIVE_NEXT.fetch_add(1, Ordering::Relaxed);
        live_registry()
            .lock()
            .map_err(|_| Error::from_reason("session registry poisoned"))?
            .insert(id, tx);
        Ok(id)
    }

    fn resolve(&mut self, _env: Env, id: u32) -> Result<u32> {
        Ok(id)
    }
}

/// Open a live page session (hydrate + keep alive). Returns a session id.
#[napi]
pub fn live_open(
    html: String,
    base_url: String,
    cookies: Option<String>,
    user_agent: Option<String>,
) -> AsyncTask<LiveOpenTask> {
    // Parent the V8 platform on the main thread before the live session's worker thread
    // creates the isolate — see `turbo_surf_render::ensure_platform`.
    crate::ensure_render_init();
    AsyncTask::new(LiveOpenTask {
        html,
        base_url,
        cookies: cookies.unwrap_or_default(),
        user_agent: user_agent.unwrap_or_default(),
    })
}

/// Run `script` in the live isolate, drain to quiescence, return `String(__RESULT||'')`.
/// Async — the script may trigger fetches to a same-process server.
pub struct LiveEvalTask {
    id: u32,
    script: String,
}

#[napi]
impl Task for LiveEvalTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<String> {
        let (reply, rx) = sync_channel(1);
        live_send(
            self.id,
            LiveJob::Eval(std::mem::take(&mut self.script), reply),
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
    AsyncTask::new(LiveEvalTask { id, script })
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

/// Close a live session: reset the binding, drop the isolate, unregister (sync).
#[napi]
pub fn live_close(id: u32) -> Result<()> {
    let tx = {
        let mut reg = live_registry()
            .lock()
            .map_err(|_| Error::from_reason("session registry poisoned"))?;
        reg.remove(&id)
    };
    if let Some(tx) = tx {
        let (reply, rx) = sync_channel(1);
        if tx.send(LiveJob::Close(reply)).is_ok() {
            let _ = rx.recv();
        }
    }
    Ok(())
}
