//! turbo-surf#99: extreme `zoom` factors on huge boxes. Chrome 156 (8078.47, headless,
//! measured on localhost) saturates the integral CSSOM metrics at its layout-unit range
//! instead of throwing; a tiny zoom on a normal box reports zero.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

const PROBE: &str = r#"(()=>{
  const out={};
  for(const id of ['a','b','c']){
    const e=document.getElementById(id);
    try{e.getBoundingClientRect();out[id]=[e.offsetWidth,e.offsetHeight,e.clientWidth,e.clientHeight,e.scrollWidth,e.scrollHeight];}
    catch(x){out[id]=String(x);}
  }
  return out;
})()"#;

#[tokio::test(flavor = "current_thread")]
async fn tiny_zoom_on_a_huge_box_saturates_like_chrome156() {
    let mut page = PageSession::open(
        "<body><div id=a style=\"zoom:1e-30;width:1e30px;height:1e30px\">x</div><div id=b style=\"width:1e30px;height:7px\"></div><div id=c style=\"zoom:1e-30;width:100px;height:10px\"></div>",
        "http://127.0.0.1/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let value: Value =
        serde_json::from_str(&page.eval_json_expression(PROBE).await.unwrap()).unwrap();
    page.close();
    let big = 33_554_432;
    assert_eq!(
        value,
        json!({
            "a": [big, big, big, big, big, big],
            "b": [33_554_428, 7, 33_554_428, 7, 33_554_428, 7],
            "c": [0, 0, 0, 0, 0, 0]
        })
    );
}

/// Chrome 156 (8078.47, headless, localhost) also caps the effective zoom at 1e6:
/// `zoom: 1e30` reports currentCSSZoom 1000000, and the box saturates the layout
/// range at that zoom (the following box sits at 12000000px, not at a float-rounded
/// offset). The saturated border-box width (Chrome 33554432 / offsetWidth 34) is
/// layout-unit saturation of padding, tracked separately (see the round 3 notes), so
/// the probe reads the zoom, the heights and the offsets.
#[tokio::test(flavor = "current_thread")]
async fn huge_zoom_is_capped_at_a_million_like_chrome156() {
    let mut page = PageSession::open(
        "<body style=\"margin:0\"><div id=a style=\"zoom:1e30;width:100px;height:10px;padding:1px\">x</div><div id=c style=\"width:100px;height:10px;padding:1px\">x</div>",
        "http://127.0.0.1/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let value: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                "['a','c'].map(id=>{const e=document.getElementById(id),r=e.getBoundingClientRect();return [r.top,r.height,e.offsetTop,e.offsetHeight,e.currentCSSZoom]})",
            )
            .await
            .unwrap(),
    )
    .unwrap();
    page.close();
    assert_eq!(
        value,
        json!([
            [0, 12_000_000, 0, 12, 1_000_000],
            [12_000_000, 12, 12_000_000, 12, 1]
        ])
    );
}
