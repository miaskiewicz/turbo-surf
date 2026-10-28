//! Geometry and hit-testing methods called on the wrong receiver throw
//! `TypeError: Illegal invocation`, as WebIDL brand checks do in Chrome 156
//! (156.0.8078.x headless, the same calls on a file:// page; an SVG-free
//! `HTMLElement` getter on <body> answers normally).
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

const CALLS: &str = r#"(() => {
  const t = (f) => { try { const v = f(); return 'ok:' + typeof v } catch (e) { return e.name + ':' + e.message } };
  const d = Object.getOwnPropertyDescriptor;
  return {
    gbcr: t(() => Element.prototype.getBoundingClientRect.call(document)),
    gcr: t(() => Element.prototype.getClientRects.call({})),
    ow: t(() => d(HTMLElement.prototype, 'offsetWidth').get.call(document.getElementById('s'))),
    cw: t(() => d(Element.prototype, 'clientWidth').get.call(document)),
    efp: t(() => Document.prototype.elementFromPoint.call(document.body, 1, 1)),
    efps: t(() => Document.prototype.elementsFromPoint.call({}, 1, 1)),
    rgb: t(() => Range.prototype.getBoundingClientRect.call({})),
    bodyow: t(() => d(HTMLElement.prototype, 'offsetWidth').get.call(document.body)),
  };
})()"#;

#[tokio::test(flavor = "current_thread")]
async fn geometry_methods_brand_check_their_receivers_like_chrome156() {
    let mut page = PageSession::open(
        "<!doctype html><body><svg id=s></svg></body>",
        "https://receivers.test/",
        "",
        "",
        30_000,
    )
    .await
    .unwrap();
    let value: Value =
        serde_json::from_str(&page.eval_json_expression(CALLS).await.unwrap()).unwrap();
    let illegal = "TypeError:Illegal invocation";
    assert_eq!(
        value,
        json!({
            "gbcr": illegal, "gcr": illegal, "ow": illegal, "cw": illegal,
            "efp": illegal, "efps": illegal, "rgb": illegal, "bodyow": "ok:number"
        })
    );
}
