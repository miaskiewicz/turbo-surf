//! The blocking load behind synchronous XMLHttpRequest (turbo-surf#138).
//!
//! `send()` on a request opened with `async = false` runs the page's ordinary
//! fetch (CSP, CORS, cookies, the HTTP cache and the blob:/data: scheme fetches)
//! to completion inside the op, so no page JavaScript, timer or event runs until
//! it returns, as in Chrome. The isolate's own event loop is the thread being
//! blocked, so the network futures register with a dedicated multi-thread runtime
//! whose worker drives their I/O and timers meanwhile.
use super::*;

/// Chrome has no timeout for a document's synchronous request; this bound keeps a
/// stalled server from wedging the isolate forever (it fails as a network error).
const SYNC_LOAD_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

fn network() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: std::sync::OnceLock<Option<tokio::runtime::Runtime>> =
        std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("turbo-surf-sync-xhr")
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
}

#[op2]
#[serde]
pub(super) fn op_fetch_sync(
    state: Rc<RefCell<OpState>>,
    #[string] url: String,
    #[string] init_json: String,
) -> FetchOut {
    let failed = |url: String| FetchOut {
        final_url: url,
        ..Default::default()
    };
    let Some(network) = network() else {
        return failed(url);
    };
    let _context = network.enter();
    let load = page_fetch(state, url.clone(), init_json);
    deno_core::futures::executor::block_on(tokio::time::timeout(SYNC_LOAD_LIMIT, load))
        .unwrap_or_else(|_| failed(url))
}
