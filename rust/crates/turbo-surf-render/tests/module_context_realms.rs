//! Existing native Page contexts own module graphs without replacing core ops.
use deno_core::{
    futures, op2, resolve_import, v8, JsRuntime, ModuleLoadOptions, ModuleLoadReferrer,
    ModuleLoadResponse, ModuleLoader, ModuleResolveResponse, ModuleSource, ModuleSourceCode,
    ModuleSpecifier, ModuleType, OpState, ResolutionKind, RuntimeOptions,
};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::task::Poll;

#[op2]
async fn op_module_context_echo(value: u32) -> u32 {
    tokio::task::yield_now().await;
    value
}

struct RetirementGate(tokio::sync::oneshot::Receiver<()>);
#[op2]
async fn op_module_context_retirement_gate(state: Rc<RefCell<OpState>>) -> bool {
    let gate = state.borrow_mut().take::<RetirementGate>();
    gate.0.await.is_ok()
}

deno_core::extension!(
    module_context_ops,
    ops = [op_module_context_echo, op_module_context_retirement_gate]
);

struct Retired;
struct CancelOnDrop(Rc<Cell<bool>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

struct Loader {
    owner: &'static str,
    held_prepare: bool,
    pump: bool,
    loads: Cell<usize>,
    held_started: Rc<Cell<bool>>,
    held_cancelled: Rc<Cell<bool>>,
}
impl Loader {
    fn new(owner: &'static str) -> Rc<Self> {
        Self::with_pending_stage(owner, false, false)
    }
    fn with_pending_stage(owner: &'static str, held_prepare: bool, pump: bool) -> Rc<Self> {
        Rc::new(Self {
            owner,
            held_prepare,
            pump,
            loads: Cell::new(0),
            held_started: Default::default(),
            held_cancelled: Default::default(),
        })
    }
}
impl ModuleLoader for Loader {
    fn pump_event_loop_during_load(&self) -> bool {
        self.pump
    }

    fn prepare_load(
        &self,
        _: &ModuleSpecifier,
        _: Option<String>,
        _: Option<String>,
        _: ModuleLoadOptions,
    ) -> Pin<Box<dyn Future<Output = Result<(), deno_error::JsErrorBox>>>> {
        if !self.held_prepare {
            return Box::pin(async { Ok(()) });
        }
        self.held_started.set(true);
        let cancel = CancelOnDrop(self.held_cancelled.clone());
        Box::pin(async move {
            let _cancel = cancel;
            std::future::pending().await
        })
    }

    fn resolve(&self, specifier: &str, referrer: &str, _: ResolutionKind) -> ModuleResolveResponse {
        let url = resolve_import(specifier, referrer)
            .map_err(|error| deno_error::JsErrorBox::type_error(error.to_string()))?;
        if url.origin().ascii_serialization() != "https://modules.test" {
            return Err(deno_error::JsErrorBox::type_error(
                "unauthenticated module origin",
            ));
        }
        Ok(url)
    }

    fn load(
        &self,
        url: &ModuleSpecifier,
        _: Option<&ModuleLoadReferrer>,
        _: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        self.loads.set(self.loads.get() + 1);
        if url.path() == "/held.js" {
            self.held_started.set(true);
            let cancel = CancelOnDrop(self.held_cancelled.clone());
            return ModuleLoadResponse::Async(Box::pin(async move {
                let _cancel = cancel;
                std::future::pending().await
            }));
        }
        let code = if url.path() == "/self-close.js" {
            "globalThis.retireModuleOwner();export const owner='closed';".to_owned()
        } else {
            format!(
                "globalThis.moduleRuns=(globalThis.moduleRuns??0)+1;export const owner='{}';export const marker=globalThis.pageMarker;",
                self.owner
            )
        };
        ModuleLoadResponse::Sync(Ok(ModuleSource::new(
            ModuleType::JavaScript,
            ModuleSourceCode::String(code.into()),
            url,
            None,
        )))
    }
}

fn runtime() -> JsRuntime {
    runtime_with_loader(None)
}

fn runtime_with_loader(loader: Option<Rc<dyn ModuleLoader>>) -> JsRuntime {
    turbo_surf_render::ensure_platform();
    JsRuntime::new(RuntimeOptions {
        extensions: vec![module_context_ops::init()],
        module_loader: loader,
        validate_import_attributes_cb: Some(Box::new(|scope, _, _| {
            if scope.get_current_context().get_slot::<Retired>().is_some() {
                let message = v8::String::new(scope, "retired module owner").unwrap();
                let error = v8::Exception::type_error(scope, message);
                scope.throw_exception(error);
            }
        })),
        ..Default::default()
    })
}

fn context(rt: &mut JsRuntime, loader: Rc<Loader>) -> v8::Global<v8::Context> {
    deno_core::scope!(scope, rt);
    let source = scope.get_current_context();
    let state = source.get_aligned_pointer_from_embedder_data(deno_core::CONTEXT_STATE_SLOT_INDEX);
    let context = v8::Context::new(scope, Default::default());
    // SAFETY: this runtime owns the shared ContextState until all these contexts drop.
    unsafe {
        context.set_aligned_pointer_in_embedder_data(deno_core::CONTEXT_STATE_SLOT_INDEX, state);
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    assert!(JsRuntime::register_module_context(scope, loader));
    v8::Global::new(scope, context)
}

fn execute(
    rt: &mut JsRuntime,
    context: &v8::Global<v8::Context>,
    code: &str,
) -> v8::Global<v8::Value> {
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    let code = v8::String::new(scope, code).unwrap();
    let script = v8::Script::compile(scope, code, None).unwrap();
    let value = script.run(scope).unwrap();
    v8::Global::new(scope, value)
}

async fn evaluate(rt: &mut JsRuntime, context: &v8::Global<v8::Context>, code: &str) -> String {
    let value = execute(rt, context, code);
    let completion = {
        deno_core::scope!(scope, rt);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        JsRuntime::scoped_resolve(scope, value)
    };
    let value = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        rt.with_event_loop_promise(completion, Default::default()),
    )
    .await
    .expect("registered module owner must make progress")
    .unwrap();
    deno_core::scope!(scope, rt);
    v8::Local::new(scope, value).to_rust_string_lossy(scope)
}

#[tokio::test(flavor = "current_thread")]
async fn module_maps_keep_same_url_authority_and_caches_in_their_actual_contexts() {
    let mut rt = runtime();
    let first_loader = Loader::new("first");
    let second_loader = Loader::new("second");
    let first = context(&mut rt, first_loader.clone());
    let second = context(&mut rt, second_loader.clone());
    execute(&mut rt, &first, "globalThis.pageMarker='first-document'");
    execute(&mut rt, &second, "globalThis.pageMarker='second-document'");
    let expression = "import('https://modules.test/shared.js').then(m=>JSON.stringify([m.owner,m.marker,moduleRuns]))";
    assert_eq!(
        evaluate(&mut rt, &first, expression).await,
        r#"["first","first-document",1]"#
    );
    assert_eq!(
        evaluate(&mut rt, &second, expression).await,
        r#"["second","second-document",1]"#
    );
    assert_eq!(
        evaluate(&mut rt, &first, expression).await,
        r#"["first","first-document",1]"#
    );
    let supplied = ModuleSpecifier::parse("https://modules.test/code-in-hand.js").unwrap();
    rt.load_side_es_module_from_code_in_context(
        &second,
        &supplied,
        "export const marker=await Promise.resolve(globalThis.pageMarker);",
    )
    .await
    .unwrap();
    assert_eq!(
        evaluate(
            &mut rt,
            &second,
            "import('https://modules.test/code-in-hand.js').then(m=>m.marker)"
        )
        .await,
        "second-document"
    );
    assert_eq!(first_loader.loads.get(), 1);
    assert_eq!(second_loader.loads.get(), 1);
    drop(first);
    drop(second);
    drop(rt);
    assert_eq!(Rc::strong_count(&first_loader), 1);
    assert_eq!(Rc::strong_count(&second_loader), 1);
}

async fn begin_held_import(rt: &mut JsRuntime, context: &v8::Global<v8::Context>, loader: &Loader) {
    execute(
        rt,
        context,
        "void import('https://modules.test/held.js').catch(()=>{})",
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        std::future::poll_fn(|cx| {
            if let Poll::Ready(Err(error)) = rt.poll_event_loop(cx, Default::default()) {
                panic!("held module setup failed: {error}");
            }
            if loader.held_started.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }),
    )
    .await
    .expect("real module loader must start before retirement");
}

#[tokio::test(flavor = "current_thread")]
async fn retiring_one_context_cancels_its_module_work_and_preserves_a_surviving_context() {
    let mut rt = runtime();
    let retired_loader = Loader::new("retired");
    let surviving_loader = Loader::new("surviving");
    let retired = context(&mut rt, retired_loader.clone());
    let surviving = context(&mut rt, surviving_loader.clone());
    begin_held_import(&mut rt, &retired, &retired_loader).await;
    assert!(!retired_loader.held_cancelled.get());
    {
        deno_core::scope!(scope, rt);
        let context = v8::Local::new(scope, &retired);
        context.set_slot(Rc::new(Retired));
        JsRuntime::detach_module_context(scope, context);
    }
    assert!(retired_loader.held_cancelled.get());
    let denied = ModuleSpecifier::parse("https://modules.test/retired.js").unwrap();
    assert!(rt
        .load_side_es_module_in_context(&retired, &denied)
        .await
        .is_err());
    assert_eq!(
        evaluate(
            &mut rt,
            &retired,
            "import('https://modules.test/shared.js').then(()=>false,error=>error.name)"
        )
        .await,
        "TypeError"
    );
    execute(
        &mut rt,
        &surviving,
        "globalThis.pageMarker='surviving-document'",
    );
    assert_eq!(evaluate(&mut rt, &surviving, "import('https://modules.test/shared.js').then(m=>JSON.stringify([m.owner,m.marker,moduleRuns]))").await, r#"["surviving","surviving-document",1]"#);
    assert_eq!(surviving_loader.loads.get(), 1);
}

fn unregistered_context(rt: &mut JsRuntime) -> v8::Global<v8::Context> {
    deno_core::scope!(scope, rt);
    let source = scope.get_current_context();
    let state = source.get_aligned_pointer_from_embedder_data(deno_core::CONTEXT_STATE_SLOT_INDEX);
    let map = source.get_aligned_pointer_from_embedder_data(deno_core::MODULE_MAP_SLOT_INDEX);
    let context = v8::Context::new(scope, Default::default());
    // SAFETY: the infrastructure realm owns both borrowed slots until runtime drop.
    unsafe {
        context.set_aligned_pointer_in_embedder_data(deno_core::CONTEXT_STATE_SLOT_INDEX, state);
        context.set_aligned_pointer_in_embedder_data(deno_core::MODULE_MAP_SLOT_INDEX, map);
    }
    v8::Global::new(scope, context)
}

fn detach(rt: &mut JsRuntime, context: &v8::Global<v8::Context>) {
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    context.set_slot(Rc::new(Retired));
    JsRuntime::detach_module_context(scope, context);
}

#[tokio::test(flavor = "current_thread")]
async fn unregistered_context_cannot_cancel_infrastructure_modules_or_core_ops() {
    let initial_loader = Loader::new("initial");
    let mut rt = runtime_with_loader(Some(initial_loader.clone()));
    let initial = {
        deno_core::scope!(scope, rt);
        let context = scope.get_current_context();
        v8::Global::new(scope, context)
    };
    let surviving_loader = Loader::new("surviving");
    let surviving = context(&mut rt, surviving_loader);
    let unregistered = unregistered_context(&mut rt);
    begin_held_import(&mut rt, &initial, &initial_loader).await;
    let denied = ModuleSpecifier::parse("https://modules.test/unregistered.js").unwrap();
    let error = rt
        .load_side_es_module_in_context(&unregistered, &denied)
        .await
        .expect_err("borrowed infrastructure slots do not register a module owner");
    assert!(error.to_string().contains("not registered"));
    assert_eq!(initial_loader.loads.get(), 1);
    detach(&mut rt, &unregistered);
    assert!(
        !initial_loader.held_cancelled.get(),
        "a borrowed unregistered map is not its owner"
    );
    let echo = rt
        .execute_script("trusted-op-capture", "Deno.core.ops.op_module_context_echo")
        .unwrap();
    {
        deno_core::scope!(scope, rt);
        let context = v8::Local::new(scope, &surviving);
        let scope = &mut v8::ContextScope::new(scope, context);
        let global = context.global(scope);
        let key = v8::String::new(scope, "trustedEcho").unwrap();
        let echo = v8::Local::new(scope, echo);
        global.set(scope, key.into(), echo);
    }
    assert_eq!(
        evaluate(
            &mut rt,
            &surviving,
            "trustedEcho(41).then(value=>String(value+1))"
        )
        .await,
        "42"
    );
    execute(
        &mut rt,
        &surviving,
        "globalThis.pageMarker='surviving-document'",
    );
    assert_eq!(
        evaluate(
            &mut rt,
            &surviving,
            "import('https://modules.test/shared.js').then(module=>module.owner)"
        )
        .await,
        "surviving"
    );
    assert!(!initial_loader.held_cancelled.get());
    drop(initial);
    drop(unregistered);
    drop(surviving);
    drop(rt);
    assert!(initial_loader.held_cancelled.get());
}

fn install_self_retirement(rt: &mut JsRuntime, context: &v8::Global<v8::Context>) {
    fn retire(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
        let context = scope.get_current_context();
        context.set_slot(Rc::new(Retired));
        let observation = context.get_slot::<RetirementWake>();
        let before = observation
            .as_ref()
            .map(|state| state.count.0.load(Ordering::Relaxed));
        JsRuntime::detach_module_context(scope, context);
        if let (Some(state), Some(before)) = (observation, before) {
            state
                .woke
                .set(state.count.0.load(Ordering::Relaxed) > before);
        }
    }
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    let function = v8::Function::new(scope, retire).unwrap();
    let name = v8::String::new(scope, "retireModuleOwner").unwrap();
    context
        .global(scope)
        .set(scope, name.into(), function.into());
}

async fn drain_module_loop(rt: &mut JsRuntime) {
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        rt.run_event_loop(Default::default()),
    )
    .await
    .expect("module retirement must drain within its bounded lifecycle")
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn self_retirement_during_module_evaluation_does_not_pin_the_closed_context() {
    let mut rt = runtime();
    let loader = Loader::new("closed");
    let context = context(&mut rt, loader.clone());
    install_self_retirement(&mut rt, &context);
    execute(
        &mut rt,
        &context,
        "void import('https://modules.test/self-close.js').catch(()=>{})",
    );
    drain_module_loop(&mut rt).await;
    {
        deno_core::scope!(scope, rt);
        let context = v8::Local::new(scope, &context);
        assert!(context.get_slot::<Retired>().is_some());
    }
    drop(context);
    for _ in 0..3 {
        rt.v8_isolate().perform_microtask_checkpoint();
        rt.v8_isolate().low_memory_notification();
        drain_module_loop(&mut rt).await;
    }
    assert_eq!(
        Rc::strong_count(&loader),
        1,
        "the inactive module map must not pin its closed context"
    );
}

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl futures::task::ArcWake for WakeCount {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.0.fetch_add(1, Ordering::Relaxed);
    }
}
struct RetirementWake {
    count: Arc<WakeCount>,
    woke: Cell<bool>,
}

fn install_delayed_retirement(
    rt: &mut JsRuntime,
    context: &v8::Global<v8::Context>,
    count: Arc<WakeCount>,
) -> (Rc<RetirementWake>, tokio::sync::oneshot::Sender<()>) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    rt.op_state().borrow_mut().put(RetirementGate(receiver));
    let echo = rt
        .execute_script(
            "trusted-op-capture",
            "Deno.core.ops.op_module_context_retirement_gate",
        )
        .unwrap();
    let observation = Rc::new(RetirementWake {
        count,
        woke: Cell::new(false),
    });
    {
        deno_core::scope!(scope, rt);
        let context = v8::Local::new(scope, context);
        context.set_slot(observation.clone());
        let scope = &mut v8::ContextScope::new(scope, context);
        let name = v8::String::new(scope, "trustedGate").unwrap();
        let echo = v8::Local::new(scope, echo);
        context.global(scope).set(scope, name.into(), echo);
    }
    install_self_retirement(rt, context);
    execute(rt, context, "trustedGate().then(()=>retireModuleOwner())");
    (observation, sender)
}

async fn await_cancelled_load(
    rt: &mut JsRuntime,
    context: &v8::Global<v8::Context>,
    count: Arc<WakeCount>,
    retire: tokio::sync::oneshot::Sender<()>,
) -> String {
    let specifier = ModuleSpecifier::parse("https://modules.test/held.js").unwrap();
    let waker = futures::task::waker(count);
    let mut cx = std::task::Context::from_waker(&waker);
    let mut load = Box::pin(rt.load_side_es_module_in_context(context, &specifier));
    assert!(
        load.as_mut().poll(&mut cx).is_pending(),
        "first poll must genuinely hold the loader"
    );
    retire
        .send(())
        .expect("retirement trigger is still pending");
    let mut result = None;
    for _ in 0..100 {
        if let Poll::Ready(value) = load.as_mut().poll(&mut cx) {
            result = Some(value);
            break;
        }
        tokio::task::yield_now().await;
    }
    result
        .expect("retirement must cancel without releasing loader or unrelated ticks")
        .expect_err("retired explicit load must reject")
        .to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn retiring_an_explicit_load_wakes_and_cancels_both_preparation_and_body() {
    for prepare in [true, false] {
        let mut rt = runtime();
        let loader = Loader::with_pending_stage("held", prepare, true);
        let owner = context(&mut rt, loader.clone());
        let survivor = context(&mut rt, Loader::new("survivor"));
        let count = Arc::new(WakeCount::default());
        let (observation, retire) = install_delayed_retirement(&mut rt, &owner, count.clone());
        let error = await_cancelled_load(&mut rt, &owner, count, retire).await;
        assert!(error.contains("retired"), "{error}");
        assert!(loader.held_started.get());
        assert!(
            loader.held_cancelled.get(),
            "held loader future must be dropped before destruction"
        );
        assert!(
            observation.woke.get(),
            "detachment must wake the waiting load directly"
        );
        assert_eq!(
            evaluate(
                &mut rt,
                &survivor,
                "import('https://modules.test/shared.js').then(m=>m.owner)"
            )
            .await,
            "survivor"
        );
    }
}
