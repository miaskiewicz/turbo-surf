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
