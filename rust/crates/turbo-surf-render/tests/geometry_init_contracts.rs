//! DOMPoint/DOMMatrix dictionary arguments, `toJSON()` and rotations against Chrome
//! 156 (156.0.8078.47, headless, the same script on a localhost page):
//! - turbo-surf#79: DOMPointInit / DOMMatrixInit members are read and converted in
//!   WebIDL order (lexicographic, DOMMatrix2DInit's before DOMMatrixInit's),
//!   non-objects are rejected and conversion errors carry Blink's context;
//! - turbo-surf#80: `DOMMatrixReadOnly.prototype.toJSON` key order;
//! - turbo-surf#81: rotations by multiples of 45/90 degrees are exact, as
//!   gfx::Transform computes them.
use serde_json::Value;
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const out={};const T=(k,f)=>{try{out[k]=f()}catch(e){out[k]=e.name+':'+e.message}};
const logged=(keys,log,prefix='')=>{const o={};for(const k of keys)Object.defineProperty(o,k,{get(){log.push(prefix+k);return {valueOf(){log.push(prefix+k+'v');return 1}}},enumerable:true});return o;};
const P=['x','y','z','w'];
const M=['a','b','c','d','e','f','m11','m12','m13','m14','m21','m22','m23','m24','m31','m32','m33','m34','m41','m42','m43','m44','is2D'];
T('fromPointOrder',()=>{const log=[];DOMPoint.fromPoint(logged(P,log));return log});
T('fromPointROOrder',()=>{const log=[];DOMPointReadOnly.fromPoint(logged(P,log));return log});
T('transformPointOrder',()=>{const log=[];new DOMMatrix().transformPoint(logged(P,log));return log});
T('matrixTransformOrder',()=>{const log=[];new DOMPoint().matrixTransform(logged(['a','m11','e','m41','is2D'],log));return log});
T('fromMatrixOrder',()=>{const log=[];try{DOMMatrix.fromMatrix(logged(M,log))}catch(e){log.push(e.name)};return log});
T('multiplyOrder',()=>{const log=[];new DOMMatrix().multiply(logged(['f','a','m22'],log));return log});
for(const [k,f] of [['fromPoint',()=>DOMPoint.fromPoint(5)],['fromPointRO',()=>DOMPointReadOnly.fromPoint('x')],['transformPoint',()=>new DOMMatrix().transformPoint(5)],['matrixTransform',()=>new DOMPoint().matrixTransform(5)],['fromMatrix',()=>DOMMatrix.fromMatrix(5)],['fromMatrixRO',()=>DOMMatrixReadOnly.fromMatrix(true)],['multiply',()=>new DOMMatrix().multiply(5)],['multiplySelf',()=>new DOMMatrix().multiplySelf(5)],['preMultiplySelf',()=>new DOMMatrix().preMultiplySelf(5)]])T('bad_'+k,f);
for(const [k,f] of [['fromPoint',()=>DOMPoint.fromPoint({x:1n})],['transformPoint',()=>new DOMMatrix().transformPoint({w:Symbol()})],['fromMatrix',()=>DOMMatrix.fromMatrix({m11:1n})],['multiply',()=>new DOMMatrix().multiply({a:Symbol()})]])T('conv_'+k,f);
T('nullish',()=>[DOMPoint.fromPoint(null).w,DOMPoint.fromPoint(undefined).w,DOMMatrix.fromMatrix(null).a,new DOMMatrix().transformPoint(null).w]);
T('json',()=>[Object.keys(new DOMMatrix().toJSON()),Object.keys(new DOMMatrixReadOnly([1,2,3,4,5,6]).toJSON()),JSON.stringify(new DOMMatrix([1,2,3,4,5,6]))]);
const s=v=>String(v);
T('rotate',()=>[90,180,270,-90,45,135,360,30].map(d=>new DOMMatrix().rotate(d).toString()));
T('rotateAxis',()=>[90,180,45].map(d=>new DOMMatrix().rotateAxisAngle(1,0,0,d).toString()));
T('rotate3',()=>new DOMMatrix().rotate(90,90,90).toString());
T('rotateVector',()=>[[1,1],[0,1],[-1,0],[0,-1],[1,0],[3,4]].map(([x,y])=>{const m=new DOMMatrix().rotateFromVector(x,y);return [m.a,m.b,m.c,m.d].map(s)}));
T('chain',()=>new DOMMatrix([1,2,3,4,5,6]).translate(1,1).scale(2).rotate(90).skewX(45).toString());
T('css',()=>['rotate(90deg)','rotate(0.25turn)','rotate(45deg)','rotate(100grad)','rotateZ(90deg)','rotate3d(0,0,1,90deg)','skew(45deg)'].map(t=>new DOMMatrix(t).toString()));
globalThis.out=out;"#;

const CHROME_156: &str = r#"{"fromPointOrder":["w","wv","x","xv","y","yv","z","zv"],"fromPointROOrder":["w","wv","x","xv","y","yv","z","zv"],"transformPointOrder":["w","wv","x","xv","y","yv","z","zv"],"matrixTransformOrder":["a","av","e","ev","m11","m11v","m41","m41v","is2D"],"fromMatrixOrder":["a","av","b","bv","c","cv","d","dv","e","ev","f","fv","m11","m11v","m12","m12v","m21","m21v","m22","m22v","m41","m41v","m42","m42v","is2D","m13","m13v","m14","m14v","m23","m23v","m24","m24v","m31","m31v","m32","m32v","m33","m33v","m34","m34v","m43","m43v","m44","m44v","TypeError"],"multiplyOrder":["a","av","f","fv","m22","m22v"],"bad_fromPoint":"TypeError:Failed to execute 'fromPoint' on 'DOMPoint': The provided value is not of type 'DOMPointInit'.","bad_fromPointRO":"TypeError:Failed to execute 'fromPoint' on 'DOMPointReadOnly': The provided value is not of type 'DOMPointInit'.","bad_transformPoint":"TypeError:Failed to execute 'transformPoint' on 'DOMMatrixReadOnly': The provided value is not of type 'DOMPointInit'.","bad_matrixTransform":"TypeError:Failed to execute 'matrixTransform' on 'DOMPointReadOnly': The provided value is not of type 'DOMMatrixInit'.","bad_fromMatrix":"TypeError:Failed to execute 'fromMatrix' on 'DOMMatrix': The provided value is not of type 'DOMMatrixInit'.","bad_fromMatrixRO":"TypeError:Failed to execute 'fromMatrix' on 'DOMMatrixReadOnly': The provided value is not of type 'DOMMatrixInit'.","bad_multiply":"TypeError:Failed to execute 'multiply' on 'DOMMatrixReadOnly': The provided value is not of type 'DOMMatrixInit'.","bad_multiplySelf":"TypeError:Failed to execute 'multiplySelf' on 'DOMMatrix': The provided value is not of type 'DOMMatrixInit'.","bad_preMultiplySelf":"TypeError:Failed to execute 'preMultiplySelf' on 'DOMMatrix': The provided value is not of type 'DOMMatrixInit'.","conv_fromPoint":"TypeError:Failed to execute 'fromPoint' on 'DOMPoint': Failed to read the 'x' property from 'DOMPointInit': Cannot convert a BigInt value to a number","conv_transformPoint":"TypeError:Failed to execute 'transformPoint' on 'DOMMatrixReadOnly': Failed to read the 'w' property from 'DOMPointInit': Cannot convert a Symbol value to a number","conv_fromMatrix":"TypeError:Failed to execute 'fromMatrix' on 'DOMMatrix': Failed to read the 'm11' property from 'DOMMatrix2DInit': Cannot convert a BigInt value to a number","conv_multiply":"TypeError:Failed to execute 'multiply' on 'DOMMatrixReadOnly': Failed to read the 'a' property from 'DOMMatrix2DInit': Cannot convert a Symbol value to a number","nullish":[1,1,1,1],"json":[["a","b","c","d","e","f","m11","m12","m13","m14","m21","m22","m23","m24","m31","m32","m33","m34","m41","m42","m43","m44","is2D","isIdentity"],["a","b","c","d","e","f","m11","m12","m13","m14","m21","m22","m23","m24","m31","m32","m33","m34","m41","m42","m43","m44","is2D","isIdentity"],"{\"a\":1,\"b\":2,\"c\":3,\"d\":4,\"e\":5,\"f\":6,\"m11\":1,\"m12\":2,\"m13\":0,\"m14\":0,\"m21\":3,\"m22\":4,\"m23\":0,\"m24\":0,\"m31\":0,\"m32\":0,\"m33\":1,\"m34\":0,\"m41\":5,\"m42\":6,\"m43\":0,\"m44\":1,\"is2D\":true,\"isIdentity\":false}"],"rotate":["matrix(0, 1, -1, 0, 0, 0)","matrix(-1, 0, 0, -1, 0, 0)","matrix(0, -1, 1, 0, 0, 0)","matrix(0, -1, 1, 0, 0, 0)","matrix(0.7071067811865476, 0.7071067811865476, -0.7071067811865476, 0.7071067811865476, 0, 0)","matrix(-0.7071067811865476, 0.7071067811865476, -0.7071067811865476, -0.7071067811865476, 0, 0)","matrix(1, 0, 0, 1, 0, 0)","matrix(0.8660254037844387, 0.49999999999999994, -0.49999999999999994, 0.8660254037844387, 0, 0)"],"rotateAxis":["matrix3d(1, 0, 0, 0, 0, 0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1)","matrix3d(1, 0, 0, 0, 0, -1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1)","matrix3d(1, 0, 0, 0, 0, 0.7071067811865476, 0.7071067811865476, 0, 0, -0.7071067811865476, 0.7071067811865476, 0, 0, 0, 0, 1)"],"rotate3":"matrix3d(0, 0, -1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1)","rotateVector":[["0.7071067811865476","0.7071067811865476","-0.7071067811865476","0.7071067811865476"],["0","1","-1","0"],["-1","0","0","-1"],["0","-1","1","0"],["1","0","0","1"],["0.6","0.8","-0.8","0.6"]],"chain":"matrix(6, 8, 3.999999999999999, 3.999999999999999, 9, 12)","css":["matrix(0, 1, -1, 0, 0, 0)","matrix(0, 1, -1, 0, 0, 0)","matrix(0.7071067811865476, 0.7071067811865476, -0.7071067811865476, 0.7071067811865476, 0, 0)","matrix(0, 1, -1, 0, 0, 0)","matrix(0, 1, -1, 0, 0, 0)","matrix3d(0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)","matrix(1, 0, 0.9999999999999999, 1, 0, 0)"]}"#;

async fn results() -> (Value, Value) {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    (got, serde_json::from_str(CHROME_156).unwrap())
}

fn assert_keys(got: &Value, chrome: &Value, select: impl Fn(&str) -> bool) {
    let mut checked = 0;
    for (key, want) in chrome.as_object().unwrap() {
        if select(key) {
            assert_eq!(got.get(key), Some(want), "{key}");
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#79: fails until geometry dictionaries convert like WebIDL"]
async fn geometry_dictionaries_convert_like_chrome156() {
    let (got, chrome) = results().await;
    assert_keys(&got, &chrome, |key| {
        key.ends_with("Order")
            || key.starts_with("bad_")
            || key.starts_with("conv_")
            || key == "nullish"
    });
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#80: fails until toJSON lists is2D/isIdentity last"]
async fn matrix_to_json_key_order_matches_chrome156() {
    let (got, chrome) = results().await;
    assert_keys(&got, &chrome, |key| key == "json");
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#81: fails until 45/90 degree rotations are exact"]
async fn rotations_match_chrome156() {
    let (got, chrome) = results().await;
    assert_keys(&got, &chrome, |key| {
        key.starts_with("rotate") || key == "chain" || key == "css"
    });
}
