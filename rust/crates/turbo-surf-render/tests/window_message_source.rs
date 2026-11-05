//! turbo-surf#85/#90: a posted message's `event.source` is the WindowProxy of the
//! sender's incumbent realm on every posting path (child eval, child timer, child
//! script, a child-realm function called from the parent, nested, sibling, self,
//! parent -> child); turbo-surf#129: `new Window()` / `Window()` messages. Expected:
//! Chrome 156 running the same probe on a localhost page
//! (`harness/detection/window-message-source-chrome-control.mjs`).
use turbo_surf_render::PageSession;

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
    // Includes the synchronous posts child-realm code makes when the top realm calls
    // into it (`a.eval(...)`, a child-realm function, nested, sibling): the source is
    // the incumbent (calling frame's) realm, not the entered one (turbo-surf#90).
    assert_eq!(actual["rows"], control["rows"]);
}

/// The natively entered `postMessage` (turbo-surf#90) is each realm's own Web IDL
/// operation, as in Chrome 156: the frame's `Function`, length 1, no prototype, not a
/// constructor, native source.
#[tokio::test(flavor = "current_thread")]
async fn post_message_entry_belongs_to_its_realm() {
    let mut page = PageSession::open("<body>", "http://127.0.0.1/", "", "", 5000)
        .await
        .unwrap();
    let probe = r#"(()=>{const f=document.createElement('iframe');document.body.append(f);
const w=f.contentWindow,p=w.postMessage;let constructs;try{new p('x');constructs=false}catch(e){constructs=e instanceof w.TypeError||e instanceof TypeError}
return [postMessage.constructor===Function,p.constructor===w.Function,p!==postMessage,p.length,p.name,
Object.hasOwn(p,'prototype'),constructs,Function.prototype.toString.call(p)];})()"#;
    let actual = page.eval_json_expression(probe).await.unwrap();
    assert_eq!(
        actual,
        r#"[true,true,true,1,"postMessage",false,true,"function postMessage() { [native code] }"]"#
    );
}
