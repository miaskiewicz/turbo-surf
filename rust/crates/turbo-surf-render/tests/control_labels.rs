use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const CASES: &str = include_str!("../src/radio_dirty_cases.json");

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
async fn native_control_labels_match_independent_chrome_and_borrowed_receiver_realms() {
    for (probe, oracle) in [
        (
            include_str!("../src/control_labels_probe.js"),
            include_str!("../src/control_labels_chrome155.json"),
        ),
        (
            include_str!("../src/control_labels_realm_probe.js"),
            include_str!("../src/control_labels_realm_chrome155.json"),
        ),
    ] {
        let expected = control(probe, oracle);
        let mut page = open().await;
        assert_eq!(
            read(&mut page, probe).await,
            expected["result"][0]["result"]
        );
        page.close();
    }
}
#[tokio::test(flavor = "current_thread")]
async fn native_control_labels_preserve_adopted_live_list_and_retired_realm_ownership() {
    let probe = include_str!("../src/control_labels_gc_probe.js");
    let oracle = include_str!("../src/control_labels_gc_chrome155.json");
    let expected = control(probe, oracle);
    assert_eq!(
        expected["gcPolicy"],
        serde_json::json!({"rounds":3,"settleMs":50,"taskExpression":"0"})
    );
    let mut page = open().await;
    assert_eq!(read(&mut page, probe).await, serde_json::json!("prepared"));
    if expected["gcPolicy"]["settleMs"].as_u64() == Some(50) {
        assert_eq!(
            read(
                &mut page,
                "__internalsLifetime.refs[1].deref().frameElement===null"
            )
            .await,
            serde_json::json!(true)
        );
    }
    for (release, phase) in [(false, "first"), (true, "afterRelease")] {
        if release {
            page.eval_json_expression("__internalsLifetime.retained=null")
                .await
                .unwrap();
        }
        let settle_ms = expected["gcPolicy"]["settleMs"].as_u64().unwrap_or(0);
        if settle_ms > 0 {
            assert_eq!(
                expected["gcPolicy"],
                serde_json::json!({"rounds":3,"settleMs":50,"taskExpression":"0"})
            );
        }
        for _ in 0..3 {
            if settle_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(settle_ms)).await;
                assert_eq!(read(&mut page, "0").await, serde_json::json!(0));
            }
            page.collect_message_port_garbage_for_testing();
        }
        assert_eq!(
            read(&mut page, "__readInternalsLifetime()").await,
            expected["result"][0]["result"][phase]
        );
    }
    page.close();
}
