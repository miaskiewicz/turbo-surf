use serde_json::{json, Value};
use turbo_surf_render::{run_with_dom, PageSession};
const HTML: &str = include_str!("fixtures/auth_menu_animations.html");
const MODAL: &str = include_str!("fixtures/auth_modal_reflow.js");
fn setup() -> String {
    format!(
        "{};{MODAL}",
        HTML.split_once("<script>")
            .unwrap()
            .1
            .split_once("</script>")
            .unwrap()
            .0
    )
}

#[tokio::test(flavor = "current_thread")]
async fn mui_modal_manager_and_fade_can_read_known_zero_offsets_with_chat_animations() {
    let html = format!("{}</body></html>", HTML.split_once("<script>").unwrap().0);
    let mut page = PageSession::open(&html, "https://modal.test/", "", "", 60_000)
        .await
        .unwrap();
    page.eval(&setup()).await.unwrap();
    for phase in ["typing", "message"] {
        page.eval(&format!("setAuthPhase('{phase}')"))
            .await
            .unwrap();
        let report: Value =
            serde_json::from_str(&page.eval_json_expression("openAuthModal()").await.unwrap())
                .unwrap();
        for key in [
            "scrollbarSize",
            "padding",
            "mountTop",
            "reflowTop",
            "reflowLeft",
            "bodyTop",
            "rootTop",
        ] {
            assert_eq!(report[key]["value"], 0, "{phase}/{key}: {report}");
        }
        assert_eq!(report["viewportOverflow"]["value"], false);
        assert_eq!(report["paperWidth"]["value"], 500);
        page.input(&json!([{"click":"#modal-action"}]))
            .await
            .unwrap();
        assert_eq!(page.eval_json_expression("modalClicks").await.unwrap(), "1");
        assert_eq!(
            page.eval_json_expression("modalRipple.width.value===modalRipple.client.value")
                .await
                .unwrap(),
            "true"
        );
        page.eval("document.getElementById('auth-modal').remove();document.body.style.removeProperty('overflow')").await.unwrap();
    }
    page.close();
}

#[test]
fn nonzero_offsets_still_clamp_and_disconnected_or_inert_elements_read_zero() {
    let html="<!doctype html><style>body{height:4000px}#scroll{height:40px;width:100px;overflow:auto}#child{height:200px}</style><div id=scroll><div id=child></div></div>";
    let actual=run_with_dom(html,"(()=>{const node=document.getElementById('scroll'),child=document.getElementById('child');window.scrollTo(0,100);node.scrollTop=80;const rows=[window.scrollY,node.scrollTop];node.remove();rows.push(node.scrollTop);document.body.append(node);child.style.height='10px';rows.push(node.scrollTop);document.body.style.height='10px';rows.push(window.scrollY);const inert=document.implementation.createHTMLDocument('inert');rows.push(inert.body.scrollTop);return JSON.stringify(rows)})()").unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&actual).unwrap(),
        json!([100, 80, 0, 0, 0, 0])
    );
    let quirks=run_with_dom("<style>body{height:4000px}</style><div>quirks</div>","(()=>{const before=document.body.scrollTop;document.body.scrollTop=100;const moved=document.body.scrollTop;window.scrollTo(0,0);return JSON.stringify([before,moved,document.body.scrollTop])})()").unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&quirks).unwrap(),
        json!([0, 100, 0])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_scroll_getter_uses_child_owner_context_not_parent_zero_state() {
    let mut page=PageSession::open("<!doctype html><iframe srcdoc=\"<!doctype html><div id='scroll' style='height:20px;overflow:auto'><div style='height:200px'></div></div>\"></iframe>","https://frame.test/","","",60_000).await.unwrap();
    let actual=page.eval_json_expression("(()=>{const child=document.querySelector('iframe').contentDocument.getElementById('scroll');child.scrollTop=70;const getter=Object.getOwnPropertyDescriptor(Element.prototype,'scrollTop').get;return [document.documentElement.scrollTop,getter.call(child),child.scrollTop]})()").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&actual).unwrap(),
        json!([0, 70, 70])
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn unchanged_zero_assignment_converts_once_and_interrupts_pending_smooth_scroll() {
    let mut page = PageSession::open(
        "<!doctype html><div id=box style='height:40px;overflow:auto'><div style='height:200px'></div></div>",
        "https://modal.test/",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let actual = page
        .eval_json_expression(
            "(async()=>{const box=document.getElementById('box');let conversions=0;const pending=box.scroll({top:60,behavior:'smooth'});box.scrollTop={valueOf(){conversions++;return 0}};const outcome=await pending;return {conversions,interrupted:outcome.interrupted,top:box.scrollTop}})()",
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&actual).unwrap(),
        json!({"conversions": 1, "interrupted": true, "top": 0})
    );
    page.close();
}
