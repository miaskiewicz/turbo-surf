//! The one-shot `render_capture_cookies*` entry points: the authored document hydrates
//! under its actual response policy, each host program between script boundaries runs
//! in order, and the result is the serialized DOM plus the earned cookie jar.
use serde_json::{json, Value};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::{render_capture_cookies_with_response_headers, SCRIPT_BOUNDARY};

const PAGE: &str = "<!doctype html><html><head></head><body><script nonce=\"ok\">document.cookie='page=1';document.body.dataset.page='ran';</script><script>document.body.dataset.blocked='ran';</script></body></html>";

#[tokio::test(flavor = "current_thread")]
async fn capture_runs_host_programs_in_order_under_the_response_policy() {
    let script = [
        "document.cookie='host=2';document.body.append('first');",
        "   ",
        "document.body.append(document.body.lastChild.data+'+second');",
    ]
    .join(SCRIPT_BOUNDARY);
    let (html, cookies) = render_capture_cookies_with_response_headers(
        PAGE,
        "https://capture.test/path/page",
        "",
        "",
        &script,
        5000,
        ClientHints::default(),
        &[(
            "Content-Security-Policy".into(),
            "script-src 'nonce-ok'".into(),
        )],
    )
    .await
    .unwrap();
    assert!(
        html.contains("<body data-page=\"ran\">"),
        "inline policy: {html}"
    );
    assert!(html.ends_with("first+second</body></html>"), "{html}");
    let mut jar: Value = serde_json::from_str(&cookies).unwrap();
    // The jar's serialization order is unspecified.
    jar.as_array_mut()
        .unwrap()
        .sort_by_key(|cookie| cookie["name"].as_str().unwrap().to_owned());
    // document.cookie without Path defaults to the document URL's directory; the
    // page's own script ran during hydration, before the host programs.
    let cookie = |name: &str, value: &str| {
        json!({"host_only":true,"name":name,"value":value,"domain":"capture.test","path":"/path",
            "secure":false,"http_only":false,"same_site":"lax","expires_at":null})
    };
    assert_eq!(jar, json!([cookie("host", "2"), cookie("page", "1")]));
}

#[tokio::test(flavor = "current_thread")]
async fn capture_reports_a_failing_host_program() {
    let error = render_capture_cookies_with_response_headers(
        "<body>",
        "https://capture.test/",
        "",
        "",
        &format!("globalThis.before=1;{SCRIPT_BOUNDARY}throw new TypeError('host program failed')"),
        5000,
        ClientHints::default(),
        &[],
    )
    .await
    .unwrap_err();
    assert!(error.contains("TypeError: host program failed"), "{error}");
}

#[test]
fn pow_engine_runs_challenge_js_against_an_empty_document() {
    use turbo_surf_core::challenge::PowEngine;
    let answer = turbo_surf_render::V8PowEngine
        .compute("String(document.body.childNodes.length)+':'+(6*7)+':'+typeof navigator.userAgent")
        .unwrap();
    assert_eq!(answer, "0:42:string");
    let error = turbo_surf_render::V8PowEngine
        .compute("throw new RangeError('no answer')")
        .unwrap_err();
    assert!(error.contains("RangeError: no answer"), "{error}");
}

#[test]
fn native_engine_version_is_the_linked_v8() {
    let version = turbo_surf_render::v8_version();
    let numbers = version
        .trim_end_matches("-rusty")
        .split('.')
        .collect::<Vec<_>>();
    assert_eq!(numbers.len(), 4, "{version}");
    assert!(
        numbers.iter().all(|part| part.parse::<u32>().is_ok()),
        "{version}"
    );
    assert_eq!(
        turbo_surf_render::stable_window_proxy(),
        cfg!(turbo_surf_native_proxy)
    );
}
