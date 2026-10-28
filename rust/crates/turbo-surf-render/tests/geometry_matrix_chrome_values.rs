//! DOMMatrix values and errors against Chrome 156 (156.0.8078.47, headless, the same
//! script on a localhost page). Each test pins one issue:
//! - turbo-surf#91: `is2D: true` with 3D members throws Blink's message per caller;
//! - turbo-surf#92: rotateAxisAngle with a NaN or infinite axis is all NaN (3x3);
//! - turbo-surf#93: the axis-aligned 2D inverse stays in doubles (reciprocal determinant).
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

const INVERSE_SCRIPT: &str = r#"const out=[];const T=f=>{try{out.push(String(f()))}catch(e){out.push(e.name+': '+e.message)}};
T(()=>new DOMMatrix([3,0,0,3,1,1]).inverse());
T(()=>new DOMMatrix([3,0,0,0,0,3,0,0,0,0,1,0,1,1,0,1]).inverse());
T(()=>new DOMMatrix([3,0,0,3,1,1]).invertSelf());
T(()=>new DOMMatrixReadOnly([1e-3,0,0,1e40,1,1]).inverse());
for(const [a,d,e,f] of [[7,0.1,0.3,0.7],[0.3,1.7,0.1,0.9],[1.1,13,0.7,0.3],[3.3,0.07,1.9,2.3],[9,11,0.123,0.456]])T(()=>{const m=new DOMMatrix([a,0,0,d,e,f]).inverse();return [m.a,m.d,m.e,m.f].join()});
globalThis.out=out;"#;

const INVERSE_CHROME_156: &str = r#"["matrix(0.3333333333333333, 0, 0, 0.3333333333333333, -0.3333333333333333, -0.3333333333333333)","matrix3d(0.3333333333333333, 0, 0, 0, 0, 0.3333333333333333, 0, 0, 0, 0, 1, 0, -0.3333333333333333, -0.3333333333333333, 0, 1)","matrix(0.3333333333333333, 0, 0, 0.3333333333333333, -0.3333333333333333, -0.3333333333333333)","matrix(999.9999999999999, 0, 0, 1e-40, -999.9999999999999, -1e-40)","0.14285714285714285,9.999999999999998,-0.04285714285714285,-6.999999999999998","3.333333333333333,0.588235294117647,-0.33333333333333337,-0.5294117647058824","0.909090909090909,0.07692307692307691,-0.6363636363636362,-0.023076923076923075","0.30303030303030304,14.285714285714285,-0.5757575757575758,-32.857142857142854","0.11111111111111112,0.09090909090909091,-0.013666666666666667,-0.04145454545454546"]"#;

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#93: fails until the fix"]
async fn axis_aligned_2d_inverse_stays_in_doubles() {
    let got = eval(INVERSE_SCRIPT).await;
    assert_eq!(got, serde_json::from_str::<Value>(INVERSE_CHROME_156).unwrap());
}
