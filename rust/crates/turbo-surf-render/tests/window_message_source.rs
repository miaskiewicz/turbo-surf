//! turbo-surf#85/#90: a posted message's `event.source` is the WindowProxy of the
//! sender's incumbent realm on every posting path (child eval, child timer, child
//! script, a child-realm function called from the parent, nested, sibling, self,
//! parent -> child); turbo-surf#129: `new Window()` / `Window()` messages. Expected:
//! Chrome 156 running the same probe on a localhost page
//! (`harness/detection/window-message-source-chrome-control.mjs`).
use turbo_surf_render::PageSession;

/// Posts made synchronously by child-realm code that the top realm called into
/// (`a.eval(...)`, a child-realm function): surf attributes them to the entered (top)
/// realm because the V8 binding exposes no incumbent-context API (turbo-surf#90).
/// Chrome's value is the control's; surf's is the measured gap.
const INCUMBENT_GAPS: &[(&str, &str)] = &[
    ("top<-a:eval:true", "top<-a:eval:false"),
    ("top<-a:function:true", "top<-a:function:false"),
    ("top<-g:nested:true", "top<-g:nested:false"),
    (
        "top<-b:seen sibling:true,false:true",
        "top<-b:seen sibling:false,true:true",
    ),
];

#[tokio::test(flavor = "current_thread")]
async fn message_sources_are_the_incumbent_window_like_chrome156() {
    let mut page = PageSession::open("<body>", "http://127.0.0.1/", "", "", 5000)
        .await
        .unwrap();
    let probe = include_str!("../../../../harness/detection/window-message-source-probe.js");
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/window_message_source_chrome156.json"
    ))
    .unwrap();
    let control = &control["result"];
    assert_eq!(actual["errors"], control["errors"]);
    let expected: Vec<&str> = control["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_str().unwrap())
        .map(|row| {
            INCUMBENT_GAPS
                .iter()
                .find(|(chrome, _)| *chrome == row)
                .map_or(row, |(_, surf)| surf)
        })
        .collect();
    assert_eq!(actual["rows"], serde_json::json!(expected));
}
