//! Relative form requests retain the document's base-uri admission policy.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn relative_form_post_uses_only_a_policy_admitted_base_and_installs_its_response() {
    for (policy, expected_path) in [
        ("base-uri 'none'", "/source/destination"),
        ("base-uri 'self'", "/chosen/destination"),
    ] {
        let server = fixture::Server::start().await;
        let mut page = PageSession::open_with_response_headers(
            "<!doctype html><head><base href='/chosen/'></head><body><form method='post' action='destination'><input name='q' value='owned & retained'></form>",
            &server.url("/source/start"),
            "", "", 5000, "", Default::default(),
            &[("Content-Security-Policy".into(), policy.into())],
        ).await.unwrap();
        page.eval("globalThis.beforeSubmission={};document.querySelector('form').submit()")
            .await
            .unwrap();
        let requests = server.requests();
        assert_eq!(
            requests.len(),
            1,
            "{policy}: unexpected requests {requests:?}"
        );
        assert_eq!(requests[0].method, "POST", "{policy}");
        assert_eq!(requests[0].target, expected_path, "{policy}");
        assert_eq!(requests[0].body, b"q=owned+%26+retained", "{policy}");
        assert_eq!(
            requests[0].headers["content-type"], "application/x-www-form-urlencoded",
            "{policy}"
        );
        let observed: Value = serde_json::from_str(
            &page.eval_json_expression("({href:location.href,title:document.title,text:document.getElementById('content').textContent,old:typeof beforeSubmission})")
                .await.unwrap(),
        ).unwrap();
        assert_eq!(
            observed,
            json!({
                "href":server.url(expected_path),
                "title":expected_path.trim_start_matches('/'),
                "text":expected_path.trim_start_matches('/'),
                "old":"undefined"
            }),
            "{policy}"
        );
        assert_eq!(page.eval_expression("6 * 7").await.unwrap(), "42");
        assert_eq!(
            server.requests().len(),
            1,
            "{policy}: healthy read fetched again"
        );
        page.close();
    }
}
