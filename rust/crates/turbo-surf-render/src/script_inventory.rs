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
    profile_request: Option<i32>,
    profile_saved: bool,
    profile_setup: BTreeMap<i32, (String, bool)>,
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
        if let Some((method, acknowledged)) = records.profile_setup.get_mut(&(id as i32)) {
            *acknowledged = value.get("result").is_some();
            if !*acknowledged {
                let method = method.clone();
                records
                    .failures
                    .push(format!("{method} failed: {}", value["error"]));
            }
            return;
        }
        if records.profile_request == Some(id as i32) {
            records.profile_request = None;
            if value["result"]["profile"].is_object() {
                let bytes = serde_json::to_vec(&value["result"]["profile"]).unwrap();
                if bytes.len() > SOURCE_LIMIT {
                    records
                        .failures
                        .push("CPU profile byte budget exceeded".into());
                } else if let Err(error) = save(&directory.join("cpu-profile.json"), &bytes) {
                    records.failures.push(error.to_string());
                } else {
                    records.profile_saved = true;
                }
            } else {
                records
                    .failures
                    .push(format!("CPU profile failed: {}", value["error"]));
            }
            return;
        }
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
    profiling: bool,
    layout_profile: Option<crate::layout_profile::LayoutProfile>,
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
        let profiling = std::env::var_os("TURBO_SURF_JS_PROFILE").is_some_and(|value| value == "1");
        if profiling {
            records.borrow_mut().profile_setup = [
                (3, ("Profiler.enable".into(), false)),
                (4, ("Profiler.setSamplingInterval".into(), false)),
                (5, ("Profiler.start".into(), false)),
            ]
            .into_iter()
            .collect();
            session.post_message(3, "Profiler.enable", None::<Value>);
            session.post_message(
                4,
                "Profiler.setSamplingInterval",
                Some(json!({"interval":1000})),
            );
            session.post_message(5, "Profiler.start", None::<Value>);
        }
        let layout_profile =
            profiling.then(|| crate::layout_profile::LayoutProfile::install(runtime.v8_isolate()));
        records.borrow_mut().sequence = 5;
        Ok(Self {
            session,
            records,
            directory,
            profiling,
            layout_profile,
        })
    }

    pub(crate) fn stop_profile(&mut self) {
        if !self.profiling {
            return;
        }
        self.profiling = false;
        let layout_profile = self.layout_profile.take().map(|profile| profile.stop());
        let id = {
            let mut records = self.records.borrow_mut();
            records.sequence += 1;
            let id = records.sequence;
            records.profile_request = Some(id);
            id
        };
        self.session
            .post_message(id, "Profiler.stop", None::<Value>);
        self.session
            .post_message(id + 1, "Profiler.disable", None::<Value>);
        self.records.borrow_mut().sequence += 1;
        if let Some(value) = layout_profile {
            if let Err(error) = save(
                &self.directory.join("layout-profile.json"),
                &serde_json::to_vec_pretty(&value).unwrap(),
            ) {
                self.records.borrow_mut().failures.push(error.to_string());
            }
        }
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
        let value = json!({"backend":"native V8 Inspector", "intrusive":true, "scripts":records.scripts.values().collect::<Vec<_>>(), "dropped":records.dropped,"sourceBytes":records.bytes,"pendingSources":records.requests.len(),"cpuProfileSaved":records.profile_saved,"cpuProfilePending":records.profile_request.is_some(),"cpuProfileSetup":{"requestedIntervalMicroseconds":(!records.profile_setup.is_empty()).then_some(1000),"commands":records.profile_setup.values().map(|(method,acknowledged)| json!({"method":method,"acknowledged":acknowledged})).collect::<Vec<_>>()},"errors":records.failures,"coverage":if cfg!(turbo_surf_native_proxy){"main and native iframe V8 contexts; native workers are unsupported"}else{"main V8 context; child inspector registration requires a source-built native proxy profile; native workers are unsupported"}});
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
        self.stop_profile();
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_profiler_setup_failure_is_retained_in_inventory_evidence() {
        let records = RefCell::new(Records::default());
        records
            .borrow_mut()
            .profile_setup
            .insert(4, ("Profiler.setSamplingInterval".into(), false));
        record(
            InspectorMsg::notification(json!({
                "id":4,"error":{"message":"Sampling interval unavailable"}
            })),
            &records,
            Path::new("."),
        );
        let records = records.borrow();
        assert!(!records.profile_setup[&4].1);
        assert_eq!(records.failures.len(), 1);
        assert!(records.failures[0].contains("Profiler.setSamplingInterval"));
    }

    fn message(value: Value) -> InspectorMsg {
        InspectorMsg::notification(value)
    }
    fn parsed(id: &str) -> InspectorMsg {
        message(json!({"method":"Debugger.scriptParsed","params":{"scriptId":id,"url":"x.js"}}))
    }

    #[test]
    fn script_records_are_bounded_and_sources_saved_by_content_hash() {
        let directory = std::env::temp_dir().join(format!("surf-inventory-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let records = RefCell::new(Records::default());
        // Non-JSON inspector traffic is ignored.
        record(
            InspectorMsg {
                kind: deno_core::InspectorMsgKind::Notification,
                content: "not json".into(),
            },
            &records,
            &directory,
        );
        record(parsed("1"), &records, &directory);
        record(parsed("2"), &records, &directory);
        record(parsed("3"), &records, &directory);
        {
            let mut records = records.borrow_mut();
            records.requests.insert(10, "1".into());
            records.requests.insert(11, "2".into());
            records.requests.insert(12, "3".into());
        }
        let source = "console.log(1)";
        record(
            message(json!({"id":10,"result":{"scriptSource":source}})),
            &records,
            &directory,
        );
        record(
            message(json!({"id":11,"error":{"message":"No script for id"}})),
            &records,
            &directory,
        );
        records.borrow_mut().bytes = SOURCE_LIMIT;
        record(
            message(json!({"id":12,"result":{"scriptSource":"x"}})),
            &records,
            &directory,
        );
        // A response for a request nobody made is ignored.
        record(message(json!({"id":99,"result":{}})), &records, &directory);
        let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        let records = records.borrow();
        assert_eq!(
            records.scripts["1"]["sourceFile"],
            format!("script-{hash}.js")
        );
        assert_eq!(records.scripts["1"]["sourceBytes"], 14);
        assert_eq!(
            fs::read_to_string(directory.join(format!("script-{hash}.js"))).unwrap(),
            source
        );
        assert_eq!(
            records.scripts["2"]["sourceError"]["message"],
            "No script for id"
        );
        assert_eq!(
            records.scripts["3"]["sourceError"],
            "source byte budget exceeded"
        );
        assert_eq!(records.dropped, 1);
        assert!(records.requests.is_empty());
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn the_script_table_and_failed_writes_are_reported_not_fatal() {
        let missing = Path::new("/nonexistent-surf-inventory-dir");
        let records = RefCell::new(Records::default());
        for id in 0..SCRIPT_LIMIT + 2 {
            record(parsed(&id.to_string()), &records, missing);
        }
        assert_eq!(records.borrow().scripts.len(), SCRIPT_LIMIT);
        assert_eq!(records.borrow().dropped, 2);
        records.borrow_mut().requests.insert(5, "0".into());
        record(
            message(json!({"id":5,"result":{"scriptSource":"a"}})),
            &records,
            missing,
        );
        records.borrow_mut().profile_request = Some(6);
        record(
            message(json!({"id":6,"result":{"profile":{"nodes":[]}}})),
            &records,
            missing,
        );
        records.borrow_mut().profile_request = Some(7);
        record(
            message(json!({"id":7,"error":{"message":"no profile"}})),
            &records,
            missing,
        );
        let records = records.borrow();
        assert_eq!(records.failures.len(), 3);
        assert!(
            records.failures[0].contains("No such file"),
            "{}",
            records.failures[0]
        );
        assert!(
            records.failures[1].contains("No such file"),
            "{}",
            records.failures[1]
        );
        assert_eq!(
            records.failures[2],
            r#"CPU profile failed: {"message":"no profile"}"#
        );
        assert!(!records.profile_saved);
        assert_eq!(records.profile_request, None);
        assert_eq!(records.bytes, 0);
    }
}
