//! DOMMatrix argument conversion against Chrome 156 (156.0.8078.47, headless, the same
//! script on a local page):
//! - translate/scale/scale3d/scaleNonUniform (and CSS lengths and scale factors) narrow
//!   their arguments to float, while is2D is decided on the doubles;
//! - a trailing `undefined` scaleY defaults to scaleX. (A non-trailing one is NaN in
//!   Chrome, but Blink's axis-aligned gfx::Transform keeps that NaN out of the other
//!   members; that propagation gap is tracked separately.)
use serde_json::Value;
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"
const out=[];const T=f=>{try{out.push(String(f()))}catch(e){out.push(e.name+': '+e.message)}};
const L=m=>m.is2D+' '+Array.from(m.toFloat64Array()).join();
T(()=>L(new DOMMatrix().translate(0.1,0.2,0.3)));
T(()=>L(new DOMMatrix([1,2,3,4,0.1,0]).translateSelf(0.1,1e40)));
T(()=>L(new DOMMatrix().translate(0,0,1e-50)));
T(()=>L(new DOMMatrix().scale(0.1,0.3,0.7,0.1,0.3,0.7)));
T(()=>L(new DOMMatrix().scaleSelf(1,1,1+1e-10)));
T(()=>L(new DOMMatrix().scale3d(0.1,0.3,0.1,0.1)));
T(()=>L(new DOMMatrix().scale3dSelf(0.3)));
T(()=>L(new DOMMatrix().scaleNonUniform(0.1,0.3)));
T(()=>L(new DOMMatrix("translate3d(0.1px,0px,0.3px) scale(0.1) matrix(1,0,0,1,0.1,0)")));
T(()=>L(new DOMMatrix("scaleX(0.3) rotate(0.1deg)")));
T(()=>L(new DOMMatrix().scale(2,undefined,undefined)));
T(()=>L(new DOMMatrix().scaleSelf(2,undefined,undefined,undefined,undefined,undefined)));
globalThis.out=out;
"#;

const CHROME_156: &str = r#"["false 1,0,0,0,0,1,0,0,0,0,1,0,0.10000000149011612,0.20000000298023224,0.30000001192092896,1","true 1,2,0,0,3,4,0,0,0,0,1,0,Infinity,Infinity,NaN,NaN","false 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1","false 0.10000000149011612,0,0,0,0,0.30000001192092896,0,0,0,0,0.699999988079071,0,0.0900000011920929,0.21000000476837144,0.21000000476837144,1","false 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1","false 0.10000000149011612,0,0,0,0,0.10000000149011612,0,0,0,0,0.10000000149011612,0,0.2700000102818012,0.0900000011920929,0.0900000011920929,1","false 0.30000001192092896,0,0,0,0,0.30000001192092896,0,0,0,0,0.30000001192092896,0,0,0,0,1","true 0.10000000149011612,0,0,0,0,0.30000001192092896,0,0,0,0,1,0,0,0,0,1","false 0.10000000149011612,0,0,0,0,0.10000000149011612,0,0,0,0,1,0,0.11000000163912774,0,0.30000001192092896,1","true 0.2999995549948971,0.0017453283658983088,0,0,-0.0005235985305754281,0.9999984769132877,0,0,0,0,1,0,0,0,0,1","true 2,0,0,0,0,2,0,0,0,0,1,0,0,0,0,1","true 2,0,0,0,0,2,0,0,0,0,1,0,0,0,0,1"]
"#;

#[tokio::test(flavor = "current_thread")]
async fn matrix_arguments_narrow_like_chrome() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    let want: Vec<String> = serde_json::from_str(CHROME_156).unwrap();
    let got: Vec<String> = serde_json::from_value(got).unwrap();
    for (index, (got, want)) in got.iter().zip(&want).enumerate() {
        assert_eq!(got, want, "case {index}");
    }
    assert_eq!(got.len(), want.len());
}
