//! Opt-in raw HTTP diagnostics. These are application-layer records, not a wire capture.
//! Set TURBO_SURF_HTTP_TRACE to a local JSONL file; it can contain session secrets.
use serde_json::Value;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

static WRITE_LOCK: Mutex<()> = Mutex::new(());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
pub(crate) struct RequestId(pub u64);

pub(crate) fn next_id() -> RequestId {
    RequestId(if std::env::var_os("TURBO_SURF_HTTP_TRACE").is_some() {
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    } else {
        0
    })
}

pub(crate) fn record(event: impl FnOnce() -> Value) {
    let Some(path) = std::env::var_os("TURBO_SURF_HTTP_TRACE") else {
        return;
    };
    let Ok(_lock) = WRITE_LOCK.lock() else {
        return;
    };
    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        let mut value = event();
        value["time_us"] = serde_json::json!(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros());
        serde_json::to_writer(&mut file, &value)?;
        writeln!(file)
    })();
    if let Err(e) = result {
        eprintln!("HTTP trace write failed: {e}");
    }
}
