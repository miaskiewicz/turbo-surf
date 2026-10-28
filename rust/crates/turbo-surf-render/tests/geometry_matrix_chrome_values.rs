//! DOMMatrix values and errors against Chrome 156 (156.0.8078.47, headless, the same
//! script on a localhost page). Each test pins one issue:
//! - turbo-surf#91: `is2D: true` with 3D members throws Blink's message per caller;
//! - turbo-surf#92: rotateAxisAngle with a NaN or infinite axis is all NaN (3x3);
use serde_json::Value;
use turbo_surf_render::PageSession;

async fn eval(script: &str) -> Value {
    let html = format!("<!doctype html><body><script>{script}</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap()
}

const IS2D_SCRIPT: &str = r#"const out=[];const T=f=>{try{out.push(String(f()))}catch(e){out.push(e.name+': '+e.message)}};
const D=()=>({is2D:true,m33:2});
T(()=>DOMMatrix.fromMatrix(D()));
T(()=>DOMMatrixReadOnly.fromMatrix(D()));
T(()=>new DOMMatrix().multiply(D()));
T(()=>new DOMMatrix().multiplySelf(D()));
T(()=>new DOMMatrix().preMultiplySelf(D()));
T(()=>new DOMPoint().matrixTransform(D()));
T(()=>DOMMatrix.fromMatrix({is2D:true,m11:2}));
globalThis.out=out;"#;

const IS2D_CHROME_156: &str = r#"["TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.","TypeError: Failed to execute 'fromMatrix' on 'DOMMatrixReadOnly': The is2D member is set to true but the input matrix is a 3d matrix.","TypeError: Failed to execute 'multiply' on 'DOMMatrixReadOnly': The is2D member is set to true but the input matrix is a 3d matrix.","TypeError: Failed to execute 'multiplySelf' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.","TypeError: Failed to execute 'preMultiplySelf' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.","TypeError: Failed to execute 'matrixTransform' on 'DOMPointReadOnly': The is2D member is set to true but the input matrix is a 3d matrix.","matrix(2, 0, 0, 1, 0, 0)"]"#;

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#91: fails until the fix"]
async fn is2d_with_3d_members_throws_blink_message() {
    let got = eval(IS2D_SCRIPT).await;
    assert_eq!(got, serde_json::from_str::<Value>(IS2D_CHROME_156).unwrap());
}

const ROTATE_NAN_SCRIPT: &str = r#"const out=[];const T=f=>{try{out.push(String(f()))}catch(e){out.push(e.name+': '+e.message)}};
const L=m=>m.is2D+' '+Array.from(m.toFloat64Array()).join();
T(()=>L(new DOMMatrix().rotateAxisAngle(NaN,0,1,90)));
T(()=>L(new DOMMatrix().rotateAxisAngleSelf(0,NaN,0,90)));
T(()=>L(new DOMMatrix().rotateAxisAngle(0,0,NaN,90)));
T(()=>L(new DOMMatrix().rotateAxisAngle(Infinity,0,0,90)));
T(()=>L(new DOMMatrix().rotateAxisAngle(0,0,0,90)));
T(()=>new DOMMatrix().rotateAxisAngleSelf(NaN,0,1,90).toString());
globalThis.out=out;"#;

const ROTATE_NAN_CHROME_156: &str = r#"["false NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,0,0,0,1","false NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,0,0,0,1","false NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,0,0,0,1","false NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,0,0,0,1","true 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1","InvalidStateError: Failed to execute 'toString' on 'DOMMatrixReadOnly': DOMMatrix cannot be serialized with NaN or Infinity values."]"#;

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#92: fails until the fix"]
async fn rotate_axis_angle_with_nan_axis_is_nan() {
    let got = eval(ROTATE_NAN_SCRIPT).await;
    assert_eq!(got, serde_json::from_str::<Value>(ROTATE_NAN_CHROME_156).unwrap());
}
