//! The expanded API inventory's interfaces against Chrome 156 (turbo-surf#165):
//! `harness/detection/interface-members-probe.js`, recorded by
//! `harness/detection/interface-members-chrome-control.mjs`. Every own prototype member
//! Chrome has is present with the same descriptor kind (getter, accessor, method, value).
//! XMLHttpRequest is surf's own (`xhr.js`); the HTML/CSSOM/Animation interfaces come from
//! turbo-dom-browser-env.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/interface-members-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/interface-members-chrome156.json");

fn divergences(name: &str, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    let Some(expected) = expected.as_object() else {
        return;
    };
    let empty = serde_json::Map::new();
    let actual = actual.as_object().unwrap_or(&empty);
    for (member, kind) in expected {
        match actual.get(member) {
            Some(found) if found == kind => {}
            Some(found) => out.push(format!("{name}.{member}: {found} (Chrome {kind})")),
            None => out.push(format!("{name}.{member}: missing (Chrome {kind})")),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn interface_members_match_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    let digest = Sha256::digest(PROBE.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        control["fixtures"]["interface-members-probe.js"], hex,
        "control was captured from a different probe"
    );
    let page = control["fixtures"]["page"].as_str().unwrap();
    let mut session = PageSession::open(page, "https://members.test/", "", "", 15000)
        .await
        .unwrap();
    let raw = session
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    session.close();
    let raw: Value = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(raw.as_str().unwrap()).unwrap();
    let mut out = Vec::new();
    for (name, expected) in control["result"].as_object().unwrap() {
        divergences(name, &actual[name], expected, &mut out);
    }
    assert_eq!(out, PENDING_REPIN, "members differing from Chrome 156");
}

/// CSS Nesting's `CSSStyleRule` members landed in turbo-dom-browser-env after the pinned
/// revision (miaskiewicz/turbo-dom#358, release/0.8.0 PR #368). The next turbo-dom repin
/// empties this list; a fixed member then fails here until its row is removed.
const PENDING_REPIN: &[&str] = &[
    r#"CSSStyleRule.cssRules: missing (Chrome "getter")"#,
    r#"CSSStyleRule.deleteRule: missing (Chrome "method")"#,
    r#"CSSStyleRule.insertRule: missing (Chrome "method")"#,
];
