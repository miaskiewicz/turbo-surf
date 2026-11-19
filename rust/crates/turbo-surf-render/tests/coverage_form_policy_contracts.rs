//! Form javascript actions retain script-src authority while owned function tasks progress.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn javascript_form_action_obeys_script_src_without_blocking_an_owned_function_timer() {
    for (policy, expected_runs) in [("script-src 'none'", 0), ("script-src 'unsafe-inline'", 1)] {
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><body><form action='javascript:void(globalThis.formActionRuns++)'></form>",
            "https://forms-policy.test/start?original=1#kept",
            "", "", 5000, "", Default::default(),
            &[("Content-Security-Policy".into(), policy.into())],
        ).await.unwrap();
        let actual: Value = serde_json::from_str(&page.eval_json_expression(r#"new Promise(resolve=>{
          globalThis.formActionRuns=0;
          const form=document.querySelector('form');
          const action=form.getAttribute('action');
          form.submit();
          const synchronousRuns=formActionRuns;
          setTimeout(()=>resolve({runs:formActionRuns,synchronousRuns,action,href:location.href,functionTurns:1}),0);
        })"#).await.unwrap()).unwrap();
        assert_eq!(
            actual,
            json!({
                "runs":expected_runs,"synchronousRuns":0,
                "action":"javascript:void(globalThis.formActionRuns++)",
                "href":"https://forms-policy.test/start?original=1#kept","functionTurns":1
            }),
            "{policy}"
        );
        assert!(
            !page.has_pending_auxiliary_work(),
            "javascript action must not admit an HTTP document navigation"
        );
        page.close();
    }
}
