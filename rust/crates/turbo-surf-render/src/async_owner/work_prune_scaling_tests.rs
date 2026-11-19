//! Observe actual pruning work while public tasks preserve Promise reaction ordering.
use super::*;
use crate::runtime::PageSession;

fn measurements(page: &mut PageSession, reset: bool) -> PruneMeasurements {
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let shared = super::super::tracker(scope).unwrap();
    let mut state = shared.borrow_mut();
    let result = state.work.prune_measurements;
    if reset {
        state.work.prune_measurements = PruneMeasurements::default();
    }
    result
}

fn assert_deferred_metadata_bound(page: &mut PageSession, counts: PruneMeasurements) {
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let shared = super::super::tracker(scope).unwrap();
    let state = shared.borrow();
    // One hook can introduce only its Promise and its parent. The last prune
    // plus the actual intervening hooks bounds retained metadata, not a timer.
    assert!(
        state.work.nodes.len() <= counts.live_after_prune + 2 * state.work.maintenance,
        "deferred metadata exceeded actual hook growth bound"
    );
}

async fn workload(size: usize, fanout: bool) -> PruneMeasurements {
    let mut page = PageSession::open(
        "<body><h1>healthy</h1>",
        "https://prune-scaling.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    measurements(&mut page, true);
    let script = include_str!("work_prune_scaling_workload.js")
        .replace("__SIZE__", &size.to_string())
        .replace("__FANOUT__", if fanout { "true" } else { "false" });
    let started = std::time::Instant::now();
    let result = page.eval_json_expression(&script).await;
    let elapsed = started.elapsed();
    let counts = measurements(&mut page, false);
    eprintln!(
        "PRUNE_WORK size={size} fanout={fanout} elapsed_us={} counts={counts:?} error={:?}",
        elapsed.as_micros(),
        result.as_ref().err()
    );
    let result = result.unwrap();
    let actual: serde_json::Value = serde_json::from_str(&result).unwrap();
    let expected = if fanout {
        3 * size + size * (size - 1) / 2
    } else {
        3 + size
    };
    assert_eq!(actual["completed"], 2);
    assert_eq!(actual["checksum"], 2 * expected);
    assert_eq!(actual["healthy"], "healthy");
    assert!(counts.calls > 0, "genuine hook traffic reaches pruning");
    assert!(counts.candidates > 0);
    assert_eq!(counts.candidates, counts.retained_candidates);
    assert_deferred_metadata_bound(&mut page, counts);
    counts
}

async fn scaling(fanout: bool) {
    let small = workload(2048, fanout).await;
    let large = workload(8192, fanout).await;
    assert!(large.candidates <= small.candidates * 10,
        "fourfold graph size must not reproduce quadratic repeated scans: small={small:?} large={large:?}");
    let largest = workload(32768, fanout).await;
    assert_eq!(
        largest.bulk_membership_lookups, 0,
        "whole-graph scans must use already-visited native nodes instead of per-node membership hashing"
    );
    let per_node_scan_budget = if fanout { 40 } else { 24 };
    assert!(
        largest.candidates <= 32768 * per_node_scan_budget,
        "large reaction bursts must amortize actual whole-graph scans: {largest:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn message_channel_fanout_reports_real_prune_scaling() {
    scaling(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn message_channel_chains_report_real_prune_scaling() {
    scaling(false).await;
}

fn prune_live_metadata(page: &mut PageSession) -> usize {
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let shared = super::super::tracker(scope).unwrap();
    let mut state = shared.borrow_mut();
    state.work.prune(scope);
    state.work.nodes.len()
}

#[tokio::test(flavor = "current_thread")]
async fn pending_graph_is_retained_then_small_graph_maintenance_recovers() {
    let mut page = PageSession::open("<body>", "https://prune-retention.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, "globalThis.pendingPromises=[];globalThis.completions=[];for(let i=0;i<8192;i++)pendingPromises.push(new Promise(resolve=>completions.push(resolve)));return pendingPromises.length").unwrap();
    assert!(
        prune_live_metadata(&mut page) >= 8192,
        "genuine pending owned promises survive maintenance"
    );
    let retained = prune_live_metadata(&mut page);
    measurements(&mut page, true);
    assert_eq!(page.eval_json_expression("(async()=>{let sum=0;for(let batch=0;batch<8;batch++){const values=await Promise.all(Array.from({length:1024},(_,i)=>Promise.resolve(i).then(x=>x+1)));sum+=values.reduce((a,b)=>a+b,0)}return sum})()").await.unwrap(), (8 * 1024 * 1025 / 2).to_string());
    let transient = measurements(&mut page, false);
    eprintln!("PRUNE_RETAINED live={retained} counts={transient:?}");
    assert!(transient.peak_nodes <= 2 * retained,
        "settled bursts must not indefinitely accumulate weak metadata beside the live pending graph: retained={retained} counts={transient:?}");
    assert!(
        prune_live_metadata(&mut page) >= 8192,
        "transient maintenance cannot retire the held genuine pending promises"
    );
    assert_eq!(page.eval_json_expression("(async()=>{const joined=Promise.all(pendingPromises);for(const resolve of completions)resolve(7);delete globalThis.pendingPromises;delete globalThis.completions;return (await joined).reduce((a,b)=>a+b,0)})()").await.unwrap(), (8192 * 7).to_string());
    assert!(
        prune_live_metadata(&mut page) < 1024,
        "settled graph metadata is reclaimed without collecting a live pending promise"
    );
    measurements(&mut page, true);
    assert_eq!(page.eval_json_expression("(async()=>{let p=Promise.resolve(0);for(let i=0;i<2048;i++)p=p.then(x=>x+1);return await p})()").await.unwrap(), "2048");
    assert!(
        measurements(&mut page, false).calls > 0,
        "small graph maintenance resumes after the large graph settles"
    );
}
