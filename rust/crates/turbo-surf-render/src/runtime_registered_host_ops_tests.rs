//! Real registered host hooks preserve their native arguments and shared cookie jar.
use super::*;
use std::sync::{Arc, Mutex};

enum Input<'a> {
    Text(&'a str),
    Number(f64),
}

fn native_call(page: &mut PageSession, name: &str, input: &[Input<'_>]) -> String {
    assert!(matches!(
        name,
        "op_measure_text" | "op_cookie_get" | "op_cookie_set"
    ));
    let _activation = page.rt.activation_key().enter();
    let selected = private_infrastructure::context(&mut page.rt).unwrap();
    deno_core::scope!(scope, &mut page.rt);
    let context = v8::Local::new(scope, selected);
    let scope = &mut v8::ContextScope::new(scope, context);
    let core = context
        .get_slot::<RuntimeCore>()
        .expect("real captured runtime core");
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = private_boundary::frame_probe::own_value(scope, core, "ops").unwrap();
    let ops = v8::Local::<v8::Object>::try_from(ops).unwrap();
    let operation = private_boundary::frame_probe::own_value(scope, ops, name).unwrap();
    let operation = v8::Local::<v8::Function>::try_from(operation).unwrap();
    let args = input
        .iter()
        .map(|value| match value {
            Input::Text(value) => v8::String::new(scope, value).unwrap().into(),
            Input::Number(value) => v8::Number::new(scope, *value).into(),
        })
        .collect::<Vec<v8::Local<v8::Value>>>();
    v8::tc_scope!(let scope,scope);
    let receiver = v8::undefined(scope);
    operation
        .call(scope, receiver.into(), &args)
        .expect("valid native host operation must succeed")
        .to_rust_string_lossy(scope)
}

struct SavedMeasure(Option<MeasureFn>);

impl SavedMeasure {
    fn take() -> Self {
        Self(MEASURE_TEXT.write().unwrap().take())
    }
}

impl Drop for SavedMeasure {
    fn drop(&mut self) {
        *MEASURE_TEXT.write().unwrap() = self.0.take();
    }
}

#[test]
fn registered_text_measurer_keeps_unicode_font_and_fractional_size_arguments() {
    const CHILD: &str = "TURBO_SURF_NATIVE_MEASURE_TEST_CHILD";
    const CASE: &str = "runtime::runtime_registered_host_ops_tests::registered_text_measurer_keeps_unicode_font_and_fractional_size_arguments";
    if std::env::var_os(CHILD).as_deref() == Some(std::ffi::OsStr::new("1")) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(text_measurer_contract());
        return;
    }
    // A host callback is process-wide. Run this contract alone so another LIB
    // test cannot observe the temporary host configuration.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CASE, "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("test {CASE} ... ok")),
        "exact child case did not pass: {stdout}"
    );
    assert!(
        stdout.contains("test result: ok. 1 passed; 0 failed;"),
        "child must run exactly one passing case: {stdout}"
    );
    assert!(
        output.status.success(),
        "child failed: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn text_measurer_contract() {
    let mut page = PageSession::open("<body>healthy", "https://native-host.test/", "", "", 5000)
        .await
        .unwrap();
    let saved = SavedMeasure::take();
    assert_eq!(
        native_call(
            &mut page,
            "op_measure_text",
            &[
                Input::Text("unused"),
                Input::Text("serif"),
                Input::Number(12.0)
            ]
        ),
        "null"
    );
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    set_measure_fn(Box::new(move |text, family, size| {
        observed
            .lock()
            .unwrap()
            .push((text.to_owned(), family.to_owned(), size));
        (text.chars().count() as f64 * size, size * 1.25)
    }));
    assert_eq!(
        native_call(
            &mut page,
            "op_measure_text",
            &[
                Input::Text("😀é"),
                Input::Text("Font A, serif"),
                Input::Number(17.5)
            ]
        ),
        "[35,21.875]"
    );
    assert_eq!(
        native_call(
            &mut page,
            "op_measure_text",
            &[
                Input::Text(""),
                Input::Text("monospace"),
                Input::Number(8.0)
            ]
        ),
        "[0,10]"
    );
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            ("😀é".to_owned(), "Font A, serif".to_owned(), 17.5),
            ("".to_owned(), "monospace".to_owned(), 8.0)
        ]
    );
    drop(saved);
    assert_eq!(
        page.eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "healthy"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn registered_cookie_hooks_share_public_jar_and_preserve_cookie_access_rules() {
    let mut page = PageSession::open(
        "<body>healthy",
        "https://native-cookie.test/app/page",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let cookies = serde_json::json!([
        {"name":"session","value":"secret","domain":"native-cookie.test","path":"/","secure":true,"http_only":true,"same_site":"lax","expires_at":null,"host_only":true},
        {"name":"app","value":"seed","domain":"native-cookie.test","path":"/app","secure":true,"http_only":false,"same_site":"lax","expires_at":null,"host_only":true},
        {"name":"other","value":"hidden","domain":"native-cookie.test","path":"/other","secure":true,"http_only":false,"same_site":"lax","expires_at":null,"host_only":true}
    ]);
    page.set_cookies(&cookies.to_string());
    assert_eq!(native_call(&mut page, "op_cookie_get", &[]), "app=seed");
    for line in [
        "session=forged; Path=/",
        "injected=secret; HttpOnly; Path=/",
        "alien=bad; Domain=other.test; Path=/",
    ] {
        native_call(&mut page, "op_cookie_set", &[Input::Text(line)]);
    }
    assert_eq!(native_call(&mut page, "op_cookie_get", &[]), "app=seed");
    native_call(
        &mut page,
        "op_cookie_set",
        &[Input::Text("app=changed; Secure; Path=/app")],
    );
    assert_eq!(native_call(&mut page, "op_cookie_get", &[]), "app=changed");
    assert_eq!(
        page.eval_expression("document.cookie").await.unwrap(),
        "app=changed"
    );
    let stored: serde_json::Value = serde_json::from_str(&page.cookies()).unwrap();
    let mut pairs = stored
        .as_array()
        .unwrap()
        .iter()
        .map(|cookie| {
            (
                cookie["name"].as_str().unwrap(),
                cookie["value"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    pairs.sort_unstable();
    assert_eq!(
        pairs,
        vec![
            ("app", "changed"),
            ("other", "hidden"),
            ("session", "secret")
        ]
    );
    assert_eq!(
        page.eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "healthy"
    );
}
