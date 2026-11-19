use super::*;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

async fn page() -> PageSession {
    PageSession::open("<input id=q>", "https://admission.test/", "", "", 60_000)
        .await
        .unwrap()
}

fn expired_guard(page: &mut PageSession) -> Watchdog {
    page.set_execution_budget_for_testing(100);
    let guard = Watchdog::start(page.rt.v8_isolate(), page.budget_ms);
    let end = Instant::now() + Duration::from_secs(5);
    // A native request need not become V8's active termination exception while idle.
    // Acknowledge the actual worker call, then let the real resource entry observe it.
    while !guard.termination_requested.load(Ordering::Acquire) {
        assert!(
            Instant::now() < end,
            "the actual session guard must request termination"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(guard.exceeded.load(Ordering::Relaxed));
    guard
}

fn finish_guard(page: &mut PageSession, guard: Watchdog) {
    assert!(guard.exceeded.load(Ordering::Relaxed));
    drop(guard);
    assert!(!Watchdog::any_active(page.rt.v8_isolate()));
    page.rt.v8_isolate().cancel_terminate_execution();
    page.set_execution_budget_for_testing(60_000);
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_resource_admission_does_not_poison_later_input() {
    let mut page = page().await;
    let key = page.rt.activation_key();
    let active = key.enter();
    let context = private_infrastructure::context(&mut page.rt).unwrap();
    {
        deno_core::scope!(scope, page.rt);
        let local = v8::Local::new(scope, &context);
        assert!(local.get_slot::<CspHandle>().is_some());
        assert!(local.get_slot::<ActiveDocument>().is_some());
        assert!(local.get_slot::<FrameRetired>().is_none());
        assert!(local
            .get_slot::<private_boundary::Infrastructure>()
            .is_none());
    }
    let guard = expired_guard(&mut page);
    run_background_resource_task_in(&mut page.rt, vec![context]);
    finish_guard(&mut page, guard);
    drop(active);
    page.input(&serde_json::json!([{"focus":"#q"},{"type":"owner"}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression("document.querySelector('#q').value")
            .await
            .unwrap(),
        "owner"
    );
}

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, name).expect("actual fixture property")
}
fn invoke<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    name: &str,
    arguments: &[v8::Local<'s, v8::Value>],
) -> v8::Local<'s, v8::Value> {
    assert!(matches!(name, "op_frame_load_attempt" | "op_create_frame"));
    let core = root.get_slot::<RuntimeCore>().unwrap();
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = own(scope, core, "ops").to_object(scope).unwrap();
    let function = v8::Local::<v8::Function>::try_from(own(scope, ops, name)).unwrap();
    let receiver = v8::undefined(scope);
    function
        .call(scope, receiver.into(), arguments)
        .expect("actual registered operation")
}
fn provisional_context(page: &mut PageSession) -> v8::Global<v8::Context> {
    let root = private_infrastructure::context(&mut page.rt).unwrap();
    deno_core::scope!(scope, page.rt);
    let root = v8::Local::new(scope, root);
    let scope = &mut v8::ContextScope::new(scope, root);
    let global = root.global(scope);
    let host = own(scope, global, "admissionHost")
        .to_object(scope)
        .unwrap();
    let attempt = invoke(
        scope,
        root,
        "op_frame_load_attempt",
        &[global.into(), host.into()],
    );
    let window = invoke(
        scope,
        root,
        "op_create_frame",
        &[global.into(), host.into(), attempt],
    )
    .to_object(scope)
    .unwrap();
    let context = window.get_creation_context(scope).unwrap();
    assert!(context.get_slot::<FrameContextMarker>().is_some());
    assert!(context.get_slot::<FrameConfigured>().is_none());
    assert!(context.get_slot::<CspHandle>().is_none());
    assert!(context.get_slot::<ActiveDocument>().is_none());
    v8::Global::new(scope, context)
}

#[tokio::test(flavor = "current_thread")]
async fn structural_resource_authority_failure_remains_fail_closed() {
    let mut page = page().await;
    page.eval("globalThis.admissionHost=document.createElement('iframe');")
        .await
        .unwrap();
    let key = page.rt.activation_key();
    let active = key.enter();
    let context = provisional_context(&mut page);
    let guard = expired_guard(&mut page);
    run_background_resource_task_in(&mut page.rt, vec![context.clone()]);
    finish_guard(&mut page, guard);
    auxiliary_navigation::retire_interrupted_frames(&mut page.rt);
    {
        deno_core::scope!(scope, page.rt);
        let local = v8::Local::new(scope, &context);
        assert!(local.get_slot::<FrameRetired>().is_some());
        assert!(!JsRuntime::op_state_from(scope)
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .iter()
            .any(|saved| v8::Local::new(scope, saved) == local));
    }
    drop(active);
    assert_eq!(
        page.read_engine("return document.querySelector('#q')!==null")
            .unwrap(),
        "true"
    );
    let error = page
        .input(&serde_json::json!([{"focus":"#q"},{"type":"owner"}]))
        .await
        .unwrap_err();
    assert!(error.contains("missing native document policy"), "{error}");
}

#[derive(Default)]
struct TimerAdmissionFence {
    armed: bool,
    selected_due: Option<f64>,
    interrupted_entry: bool,
    entry_failed: bool,
    caught_termination: bool,
    admission_failed: bool,
    guard: Option<Watchdog>,
}

pub(super) fn timer_admission_fence(scope: &mut v8::PinScope, due: f64) {
    if !scope
        .get_slot::<TimerAdmissionFence>()
        .is_some_and(|fence| fence.armed)
    {
        return;
    }
    let guard = Watchdog::start(scope, 100);
    let end = Instant::now() + Duration::from_secs(5);
    while !guard.termination_requested.load(Ordering::Acquire) {
        assert!(
            Instant::now() < end,
            "actual timer guard must request termination"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let fence = scope.get_slot_mut::<TimerAdmissionFence>().unwrap();
    fence.armed = false;
    fence.selected_due = Some(due);
    fence.guard = Some(guard);
}

pub(super) fn observe_timer_admission_error(scope: &mut v8::PinScope, caught_termination: bool) {
    let rawstop = scope.is_execution_terminating();
    if let Some(fence) = scope.get_slot_mut::<TimerAdmissionFence>() {
        fence.entry_failed = true;
        fence.caught_termination = caught_termination;
        fence.interrupted_entry = rawstop;
    }
}

pub(super) fn observe_timer_caller_error(scope: &mut v8::PinScope) {
    if let Some(fence) = scope.get_slot_mut::<TimerAdmissionFence>() {
        fence.admission_failed = true;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_timer_admission_does_not_poison_later_input() {
    let mut page = page().await;
    page.read_engine(
        "globalThis.timerAdmissionRuns=0;setTimeout(()=>timerAdmissionRuns++,0);return 'armed'",
    )
    .unwrap();
    let key = page.rt.activation_key();
    let active = key.enter();
    let context = private_infrastructure::context(&mut page.rt).unwrap();
    {
        deno_core::scope!(scope, page.rt);
        let local = v8::Local::new(scope, &context);
        assert!(local.get_slot::<CspHandle>().is_some());
        assert!(local.get_slot::<ActiveDocument>().is_some());
        assert!(local.get_slot::<FrameRetired>().is_none());
        assert!(local
            .get_slot::<private_boundary::Infrastructure>()
            .is_none());
    }
    page.rt.v8_isolate().set_slot(TimerAdmissionFence {
        armed: true,
        ..Default::default()
    });
    let turn = run_next_window_timer_in(&mut page.rt, true, vec![context]);
    assert_eq!(turn.fired, 0);
    drop(turn);
    let mut fence = page
        .rt
        .v8_isolate()
        .remove_slot::<TimerAdmissionFence>()
        .unwrap();
    assert!(fence.selected_due.unwrap().is_finite());
    finish_guard(&mut page, fence.guard.take().unwrap());
    eprintln!(
        "TIMER_ADMISSION_NATIVE entry_failed={} rawstop={} caught_termination={} caller_failed={}",
        fence.entry_failed,
        fence.interrupted_entry,
        fence.caught_termination,
        fence.admission_failed
    );
    drop(active);
    assert_eq!(page.read_engine("return timerAdmissionRuns").unwrap(), "0");
    let recovery = page
        .input(&serde_json::json!([{"focus":"#q"},{"type":"owner"}]))
        .await;
    eprintln!("TIMER_ADMISSION_RECOVERY ok={}", recovery.is_ok());
    assert!(fence.entry_failed && (fence.interrupted_entry || fence.caught_termination) && fence.admission_failed,
        "actual interrupted admission precondition: entry={} raw={} caller={}; recovery={recovery:?}", fence.entry_failed, fence.interrupted_entry, fence.admission_failed);
    recovery.unwrap();
    assert_eq!(
        page.eval_expression("timerAdmissionRuns").await.unwrap(),
        "1"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn structural_timer_authority_failure_remains_fail_closed() {
    let mut page = page().await;
    page.eval("globalThis.admissionHost=document.createElement('iframe');")
        .await
        .unwrap();
    let key = page.rt.activation_key();
    let active = key.enter();
    let context = provisional_context(&mut page);
    {
        deno_core::scope!(scope, page.rt);
        let local = v8::Local::new(scope, &context);
        // Admission-level negative: this provisional realm is not a runnable timer.
        let turn = admit_window_timer(scope, local);
        assert_eq!(turn.fired, 0);
    }
    auxiliary_navigation::retire_interrupted_frames(&mut page.rt);
    drop(active);
    assert_eq!(
        page.read_engine("return document.querySelector('#q')!==null")
            .unwrap(),
        "true"
    );
    let error = page
        .input(&serde_json::json!([{"focus":"#q"},{"type":"owner"}]))
        .await
        .unwrap_err();
    assert!(error.contains("missing native document policy"), "{error}");
}
