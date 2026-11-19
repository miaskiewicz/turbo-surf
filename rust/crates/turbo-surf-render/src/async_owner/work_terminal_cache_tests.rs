//! A real native terminal witness is immutable; pending and unknown are never cached as settled.
use super::*;
use crate::runtime::{auxiliary_pages, PageSession};
use turbo_dom_browser_env::host_internals::AuxiliaryContextId;

macro_rules! actual_promise {
    ($page:ident, $root:ident, $scope:ident, $work:ident, $promise:ident) => {
        let _activation = $page.rt.activation_key().enter();
        deno_core::scope!($scope, &mut $page.rt);
        let context = auxiliary_pages::context($scope, AuxiliaryContextId($root)).unwrap();
        let $scope = &mut v8::ContextScope::new($scope, context);
        let key = v8::String::new($scope, "measuredPromise").unwrap();
        let value = context.global($scope).get($scope, key.into()).unwrap();
        let $promise = v8::Local::<v8::Promise>::try_from(value).unwrap();
        let shared = super::super::tracker($scope).unwrap();
        let mut state = shared.borrow_mut();
        let $work = &mut state.work;
    };
}

fn actual_node(
    scope: &mut v8::PinScope,
    work: &PromiseWork,
    promise: v8::Local<v8::Promise>,
) -> u32 {
    let key = v8::Local::new(scope, &work.key);
    let id = promise
        .get_private(scope, key)
        .unwrap()
        .uint32_value(scope)
        .unwrap();
    assert!(
        work.nodes[&id].owner.is_some(),
        "real document Promise has an authenticated native owner"
    );
    id
}

async fn page() -> PageSession {
    PageSession::open(
        "<body>",
        "https://promise-terminal-cache.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_fulfilled_graph_queries_reuse_the_actual_terminal_witness() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(
        root,
        "globalThis.measuredPromise=Promise.resolve(41);return 'ready'",
    )
    .unwrap();
    actual_promise!(page, root, scope, work, promise);
    let id = actual_node(scope, work, promise);
    assert_eq!(work.state(scope, id), Some(v8::PromiseState::Fulfilled));
    work.nodes[&id].native_state_observations.set(0);
    for _ in 0..2048 {
        assert_eq!(work.state(scope, id), Some(v8::PromiseState::Fulfilled));
        assert_eq!(work.node(scope, promise), Some(id));
    }
    assert_eq!(
        work.nodes[&id].native_state_observations.get(),
        0,
        "settled graph lookups and genuine hook-node lookups must not reread native Promise state"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pending_observations_remain_live_until_the_real_promise_settles() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, "globalThis.measuredPromise=new Promise(resolve=>{globalThis.completeMeasured=resolve});return 'pending'").unwrap();
    {
        actual_promise!(page, root, scope, work, promise);
        let id = actual_node(scope, work, promise);
        work.nodes[&id].native_state_observations.set(0);
        for _ in 0..64 {
            assert_eq!(work.state(scope, id), Some(v8::PromiseState::Pending));
        }
        assert_eq!(work.nodes[&id].native_state_observations.get(), 64);
        assert!(work.nodes[&id].terminal.get().is_none());
    }
    page.read_target_engine(
        root,
        "completeMeasured(42);completeMeasured(99);return 'settled'",
    )
    .unwrap();
    actual_promise!(page, root, scope, work, promise);
    let id = actual_node(scope, work, promise);
    assert_eq!(promise.state(), v8::PromiseState::Fulfilled);
    assert_eq!(work.state(scope, id), Some(v8::PromiseState::Fulfilled));
    assert_eq!(promise.result(scope).integer_value(scope), Some(42));
    work.nodes[&id].native_state_observations.set(0);
    for _ in 0..64 {
        assert_eq!(work.state(scope, id), Some(v8::PromiseState::Fulfilled));
    }
    assert_eq!(work.nodes[&id].native_state_observations.get(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_terminal_state_is_preserved_without_becoming_fulfilled() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, "globalThis.measuredPromise=Promise.reject(17);measuredPromise.catch(()=>{});return 'rejected'").unwrap();
    actual_promise!(page, root, scope, work, promise);
    let id = actual_node(scope, work, promise);
    assert_eq!(work.state(scope, id), Some(v8::PromiseState::Rejected));
    assert_eq!(promise.result(scope).integer_value(scope), Some(17));
    work.nodes[&id].native_state_observations.set(0);
    for _ in 0..64 {
        assert_eq!(work.state(scope, id), Some(v8::PromiseState::Rejected));
    }
    assert_eq!(work.nodes[&id].native_state_observations.get(), 0);
}
