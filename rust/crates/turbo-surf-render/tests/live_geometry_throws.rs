//! turbo-surf#31: live getBoundingClientRect / getComputedStyle / elementFromPoint on
//! table rows and sections, shadow-content cells and visually-hidden (sr-only) spans.
//! Chrome 156 never throws for any of them (tests/fixtures/live_geometry_throws_*;
//! collector tests/fixture-gen/live-geometry-throws-chrome.mjs).
//!
//! The throws came from turbo-dom-layout, not from surf's glue: table rows got a box
//! fragment without used-box metrics, table row groups got no fragment at all, and an
//! absolutely positioned inline was not blockified, which failed the whole snapshot.
//! turbo-dom-layout fixed all three (P5b-W1); any new throw fails this test.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("fixtures/live_geometry_throws_probe.js");
const CASES: &[u8] = include_bytes!("fixtures/live_geometry_throws_cases.json");
const CONTROL: &str = include_str!("fixtures/live_geometry_throws_chrome156.json");

/// (case, element label or "*" for every element, message): getBoundingClientRect
/// and the elementFromPoint read both throw it; getComputedStyle never throws.
/// Empty since turbo-dom-layout's P5b-W1 fixes (row and row-group boxes with used box
/// metrics, blockified positioned inlines, per-element inline failures).
const KNOWN_GAPS: &[(&str, &str, &str)] = &[];

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn thrown(entry: &Value) -> Option<&str> {
    (entry[0] == "throw").then(|| entry[2].as_str().unwrap())
}

async fn rows(html: &str) -> Vec<Value> {
    let mut page = PageSession::open(html, "https://live-geometry.test/", "", "", 10000)
        .await
        .unwrap();
    let rows: Value = serde_json::from_str(
        &page
            .eval_json_expression(PROBE.trim_end().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    rows.as_array().unwrap().clone()
}

#[tokio::test(flavor = "current_thread")]
async fn geometry_reads_that_chrome156_answers_throw_only_where_known() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(control["probeSha256"], sha(PROBE.as_bytes()));
    assert_eq!(control["casesSha256"], sha(CASES));
    let cases: Value = serde_json::from_slice(CASES).unwrap();
    let cases = cases.as_array().unwrap();
    let expected = control["result"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    let mut gaps = Vec::new();
    for (case, chrome) in cases.iter().zip(expected) {
        let id = case["id"].as_str().unwrap();
        assert_eq!(chrome["id"], id);
        // Chrome answers every read on every element.
        for row in chrome["result"].as_array().unwrap() {
            for entry in &row.as_array().unwrap()[1..] {
                assert_eq!(entry[0], "ok", "{id}: Chrome threw {row}");
            }
        }
        let rows = rows(case["html"].as_str().unwrap()).await;
        let mut throwing = Vec::new();
        for row in &rows {
            assert_eq!(thrown(&row[2]), None, "{id}: getComputedStyle threw {row}");
            assert_eq!(thrown(&row[1]), thrown(&row[3]), "{id}: {row}");
            if let Some(message) = thrown(&row[1]) {
                throwing.push((row[0].as_str().unwrap().to_owned(), message.to_owned()));
            }
        }
        if !throwing.is_empty() && throwing.len() == rows.len() {
            let message = &throwing[0].1;
            if throwing.iter().all(|(_, other)| other == message) {
                throwing = vec![("*".to_owned(), message.clone())];
            }
        }
        gaps.extend(
            throwing
                .into_iter()
                .map(|(element, message)| (id.to_owned(), element, message)),
        );
    }
    let known: Vec<_> = KNOWN_GAPS
        .iter()
        .map(|(id, element, message)| (id.to_string(), element.to_string(), message.to_string()))
        .collect();
    assert_eq!(
        gaps, known,
        "a live geometry gap changed: update KNOWN_GAPS"
    );
}
