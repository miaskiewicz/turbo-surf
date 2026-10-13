//! Opt-in native debugger inventory. It observes compiled sources without JS wrappers.
use deno_core::{
    v8, InspectorMsg, InspectorSessionKind, JsRuntime, JsRuntimeInspector, LocalInspectorSession,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

const SCRIPT_LIMIT: usize = 10_000;
const SOURCE_LIMIT: usize = 64 * 1024 * 1024;
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct Records {
    scripts: BTreeMap<String, Value>,
    requested: BTreeSet<String>,
    requests: BTreeMap<i32, String>,
    failures: Vec<String>,
    dropped: usize,
    bytes: usize,
    sequence: i32,
}

fn save(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

fn record(message: InspectorMsg, records: &RefCell<Records>, directory: &Path) {
    let Ok(value) = serde_json::from_str::<Value>(&message.content) else {
        return;
    };
    let mut records = records.borrow_mut();
    if matches!(
        value["method"].as_str(),
        Some("Debugger.scriptParsed" | "Debugger.scriptFailedToParse")
    ) {
        let params = &value["params"];
        if let Some(id) = params["scriptId"].as_str() {
            if records.scripts.len() < SCRIPT_LIMIT {
                records.scripts.insert(id.to_owned(), params.clone());
            } else {
                records.dropped += 1;
            }
        }
    } else if let Some(id) = value["id"].as_i64() {
        let Some(script_id) = records.requests.remove(&(id as i32)) else {
            return;
        };
        if let Some(source) = value["result"]["scriptSource"].as_str() {
            let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
            if records.bytes + source.len() > SOURCE_LIMIT {
                records.dropped += 1;
                records.scripts.get_mut(&script_id).unwrap()["sourceError"] =
                    json!("source byte budget exceeded");
            } else {
                let name = format!("script-{hash}.js");
                match save(&directory.join(&name), source.as_bytes()) {
                    Ok(()) => {
                        records.bytes += source.len();
                        let row = records.scripts.get_mut(&script_id).unwrap();
                        row["sourceFile"] = json!(name);
                        row["sourceHash"] = json!(hash);
                        row["sourceBytes"] = json!(source.len());
                    }
                    Err(error) => records.failures.push(error.to_string()),
                }
            }
        } else {
            records.scripts.get_mut(&script_id).unwrap()["sourceError"] = value["error"].clone();
        }
    }
}

#[cfg(turbo_surf_native_proxy)]
struct FrameInspector(Rc<JsRuntimeInspector>);

pub(crate) fn register_frame(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) {
    #[cfg(turbo_surf_native_proxy)]
    if let Some(inspector) = JsRuntime::op_state_from(scope)
        .borrow()
        .try_borrow::<FrameInspector>()
    {
        inspector.0.context_created(context, "native iframe");
    }
    #[cfg(not(turbo_surf_native_proxy))]
    let _ = (scope, context);
}

pub(crate) fn retire_frame(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) {
    #[cfg(turbo_surf_native_proxy)]
    {
        let inspector = JsRuntime::op_state_from(scope)
            .borrow()
            .try_borrow::<FrameInspector>()
            .map(|inspector| inspector.0.clone());
        if let Some(inspector) = inspector {
            let context = v8::Global::new(scope, context);
            inspector.context_destroyed(scope, context);
        }
    }
    #[cfg(not(turbo_surf_native_proxy))]
    let _ = (scope, context);
}

pub(crate) struct ScriptInventory {
    session: LocalInspectorSession,
    records: Rc<RefCell<Records>>,
    directory: PathBuf,
}

impl ScriptInventory {
    pub(crate) fn start(runtime: &mut JsRuntime, root: &Path) -> Result<Self, String> {
        let directory = root.join(format!("isolate-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        #[cfg(turbo_surf_native_proxy)]
        runtime
            .op_state()
            .borrow_mut()
            .put(FrameInspector(runtime.inspector()));
        let records = Rc::new(RefCell::new(Records::default()));
        let callback_records = records.clone();
        let callback_directory = directory.clone();
        let mut session = JsRuntimeInspector::create_local_session(
            runtime.inspector(),
            Box::new(move |message| record(message, &callback_records, &callback_directory)),
            InspectorSessionKind::NonBlocking {
                wait_for_disconnect: false,
            },
        );
        session.post_message(1, "Debugger.enable", None::<Value>);
        session.post_message(2, "Debugger.setSkipAllPauses", Some(json!({"skip":true})));
        records.borrow_mut().sequence = 2;
        Ok(Self {
            session,
            records,
            directory,
        })
    }

    pub(crate) fn flush(&mut self) {
        loop {
            let pending: Vec<_> = {
                let records = self.records.borrow();
                records
                    .scripts
                    .keys()
                    .filter(|id| !records.requested.contains(*id))
                    .cloned()
                    .collect()
            };
            if pending.is_empty() {
                break;
            }
            for script_id in pending {
                let id = {
                    let mut records = self.records.borrow_mut();
                    records.sequence += 1;
                    let id = records.sequence;
                    records.requested.insert(script_id.clone());
                    records.requests.insert(id, script_id.clone());
                    id
                };
                self.session.post_message(
                    id,
                    "Debugger.getScriptSource",
                    Some(json!({"scriptId":script_id})),
                );
            }
        }
        let records = self.records.borrow();
        let value = json!({"backend":"native V8 Inspector", "intrusive":true, "scripts":records.scripts.values().collect::<Vec<_>>(), "dropped":records.dropped,"sourceBytes":records.bytes,"pendingSources":records.requests.len(),"errors":records.failures,"coverage":if cfg!(turbo_surf_native_proxy){"main and native iframe V8 contexts; native workers are unsupported"}else{"main V8 context; child inspector registration requires a source-built native proxy profile; native workers are unsupported"}});
        if let Err(error) = save(
            &self.directory.join("inventory.json"),
            serde_json::to_string_pretty(&value).unwrap().as_bytes(),
        ) {
            eprintln!("native script inventory: {error}");
        }
    }
}

impl Drop for ScriptInventory {
    fn drop(&mut self) {
        self.flush();
    }
}
