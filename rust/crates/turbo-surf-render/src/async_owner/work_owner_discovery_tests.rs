//! Native identities can reuse only an existing authenticated owner, never an unknown lease.
use super::*;
use crate::runtime::{auxiliary_pages, private_boundary, PageSession};
use turbo_dom_browser_env::host_internals::AuxiliaryContextId;

async fn page() -> PageSession {
    PageSession::open("<body>", "https://owner-discovery.test/", "", "", 5000)
        .await
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn infrastructure_promises_remain_unknown_and_rediscover_on_resolution() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    {
        let infrastructure = page.rt.main_context();
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, &mut page.rt);
        let context = v8::Local::new(scope, infrastructure);
        assert!(context
            .get_slot::<private_boundary::Infrastructure>()
            .is_some());
        let scope = &mut v8::ContextScope::new(scope, context);
        let shared = super::super::tracker(scope).unwrap();
        assert!(
            super::super::current_owner(scope, &shared).is_none(),
            "actual infrastructure entry has no authenticated document token"
        );
        let resolver = v8::PromiseResolver::new(scope).unwrap();
        let promise = resolver.get_promise(scope);
        assert!(shared
            .borrow()
            .work
            .recorded_owner(scope, promise)
            .is_none());
        let before = shared
            .borrow()
            .work
            .prune_measurements
            .promise_owner_discoveries;
        let value = v8::Integer::new(scope, 42);
        assert_eq!(resolver.resolve(scope, value.into()), Some(true));
        let state = shared.borrow();
        assert!(
            state.work.prune_measurements.promise_owner_discoveries > before,
            "unknown owner resolution is reauthenticated, not negatively cached"
        );
        assert!(
            state.work.recorded_owner(scope, promise).is_none(),
            "an infrastructure Promise never acquires the selected Root implicitly"
        );
    }
    assert_eq!(page.read_target_engine(root, "return 42").unwrap(), "42");
}

#[tokio::test(flavor = "current_thread")]
async fn pruned_settled_promise_identity_can_attach_new_document_owned_reactions() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(
        root,
        "globalThis.retainedPromise=Promise.resolve(40);return 'armed'",
    )
    .unwrap();
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, &mut page.rt);
        let context = auxiliary_pages::context(scope, AuxiliaryContextId(root)).unwrap();
        let scope = &mut v8::ContextScope::new(scope, context);
        let key = v8::String::new(scope, "retainedPromise").unwrap();
        let value = context.global(scope).get(scope, key.into()).unwrap();
        let promise = v8::Local::<v8::Promise>::try_from(value).unwrap();
        let shared = super::super::tracker(scope).unwrap();
        let mut state = shared.borrow_mut();
        assert!(state.work.recorded_owner(scope, promise).is_some());
        state.work.prune(scope);
        assert!(
            state.work.recorded_owner(scope, promise).is_none(),
            "real settled metadata is absent after actual maintenance"
        );
        state.work.prune_measurements.promise_owner_discoveries = 0;
    }
    assert_eq!(
        page.eval_json_expression("retainedPromise.then(value=>value+2)")
            .await
            .unwrap(),
        "42"
    );
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, &mut page.rt);
        let shared = super::super::tracker(scope).unwrap();
        assert!(
            shared
                .borrow()
                .work
                .prune_measurements
                .promise_owner_discoveries
                > 0,
            "new reactions still take the genuine initial authentication path"
        );
    }
    assert_eq!(
        page.read_target_engine(root, "return typeof retainedPromise.then")
            .unwrap(),
        "\"function\""
    );
}
