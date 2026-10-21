use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const CASES: &str = include_str!("../src/form_collections_cases.json");

fn control(probe: &str, oracle: &str) -> Value {
    let value: Value = serde_json::from_str(oracle).unwrap();
    assert_eq!(value["errors"], serde_json::json!([]));
    assert_eq!(value["browser"], "Chrome/155.0.8059.80");
    assert_eq!(value["v8"], "15.5.35.23");
    for (key, source) in [("casesSha256", CASES), ("probeSha256", probe)] {
        assert_eq!(
            value[key],
            format!("{:x}", Sha256::digest(source.as_bytes()))
        );
    }
    value
}
async fn open() -> PageSession {
    let cases: Value = serde_json::from_str(CASES).unwrap();
    PageSession::open(
        cases[0]["html"].as_str().unwrap(),
        "http://localhost/control",
        "",
        "",
        15000,
    )
    .await
    .unwrap()
}
async fn read(page: &mut PageSession, source: &str) -> Value {
    serde_json::from_str(
        &page
            .eval_json_expression(source.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap()
}
#[tokio::test(flavor = "current_thread")]
async fn form_collections_use_real_child_realm_brands_and_retained_objects() {
    let probe = include_str!("../src/form_collections_realm_probe.js");
    let expected = control(
        probe,
        include_str!("../src/form_collections_realm_chrome155.json"),
    );
    let mut page = open().await;
    assert_eq!(
        read(&mut page, probe).await,
        expected["result"][0]["result"]
    );
    page.close();
}
#[tokio::test(flavor = "current_thread")]
async fn retained_form_collection_keeps_only_its_own_guest_realm() {
    let probe = include_str!("../src/form_collections_lifecycle_probe.js");
    let expected = control(
        probe,
        include_str!("../src/form_collections_lifecycle_chrome155.json"),
    );
    let mut page = open().await;
    assert_eq!(read(&mut page, probe).await, serde_json::json!("prepared"));
    for (release, phase) in [(false, "first"), (true, "afterRelease")] {
        if release {
            page.eval_json_expression("__internalsLifetime.retained=null")
                .await
                .unwrap();
        }
        for _ in 0..3 {
            page.collect_message_port_garbage_for_testing();
        }
        assert_eq!(
            read(&mut page, "__readInternalsLifetime()").await,
            expected["result"][0]["result"][phase]
        );
    }
    page.close();
}
