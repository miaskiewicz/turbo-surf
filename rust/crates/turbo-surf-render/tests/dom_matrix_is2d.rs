//! DOMMatrix `is2D` is state, not derived from the values (turbo-surf#89): like
//! Blink's `is2d_` flag it is set by the constructor input (a 6- or 16-value sequence,
//! a CSS transform list containing a 3D function, a DOMMatrixInit's `is2D`), cleared by
//! 3D operations even when the resulting values are 2D-shaped (and never set again by
//! later 2D-shaped results), propagated by multiply, kept by 2D operations and structured
//! clone, and it decides `toString()`'s `matrix` / `matrix3d` form and `toJSON().is2D`.
//! Each case reports `[is2D, toString() or its error name, toJSON().is2D, constructor]`.
//! Expected values: Chrome 156 (156.0.8078.47, headless) running the same script on a
//! localhost page.
use serde_json::Value;
use turbo_surf_render::PageSession;

const SCRIPT: &str = r##"const out={};const T=(k,f)=>{try{const m=f();out[k]=[m.is2D,(()=>{try{return m.toString()}catch(e){return e.name}})(),m.toJSON().is2D,m.constructor.name]}catch(e){out[k]=e.name+':'+e.message}};
const M=()=>new DOMMatrix();
T('none',()=>new DOMMatrix());
T('empty',()=>new DOMMatrix(''));
T('noneStr',()=>new DOMMatrix('none'));
T('rotate3dZ',()=>new DOMMatrix('rotate3d(0,0,1,90deg)'));
T('rotateZ',()=>new DOMMatrix('rotateZ(90deg)'));
T('rotateX0',()=>new DOMMatrix('rotateX(0deg)'));
T('rotateY0',()=>new DOMMatrix('rotateY(0deg)'));
T('translate3d0',()=>new DOMMatrix('translate3d(1px,2px,0)'));
T('translateZ0',()=>new DOMMatrix('translateZ(0)'));
T('scale3d1',()=>new DOMMatrix('scale3d(2,3,1)'));
T('scaleZ1',()=>new DOMMatrix('scaleZ(1)'));
T('matrix3d2d',()=>new DOMMatrix('matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,5,6,0,1)'));
T('matrix2d',()=>new DOMMatrix('matrix(1,0,0,1,5,6)'));
T('perspective',()=>new DOMMatrix('perspective(100px)'));
T('mixed',()=>new DOMMatrix('translate(1px) translateZ(0px) rotate(10deg)'));
T('ro3dstr',()=>new DOMMatrixReadOnly('rotate3d(0,0,1,90deg)'));
T('seq6',()=>new DOMMatrix([1,0,0,1,0,0]));
T('seq16id',()=>new DOMMatrix([1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]));
T('f32_16',()=>DOMMatrix.fromFloat32Array(new Float32Array([1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1])));
T('f64_6',()=>DOMMatrixReadOnly.fromFloat64Array(new Float64Array([1,0,0,1,0,0])));
T('dictNone',()=>DOMMatrix.fromMatrix({}));
T('dictFalse',()=>DOMMatrix.fromMatrix({is2D:false}));
T('dictTrue',()=>DOMMatrix.fromMatrix({is2D:true,a:2}));
T('dict3d',()=>DOMMatrix.fromMatrix({m33:2}));
T('dictM33one',()=>DOMMatrix.fromMatrix({m33:1,m13:0}));
T('dictMinus0',()=>DOMMatrix.fromMatrix({m13:-0}));
T('dictNaN',()=>DOMMatrix.fromMatrix({m13:NaN}));
T('roDictFalse',()=>DOMMatrixReadOnly.fromMatrix({is2D:false}));
T('translateZ',()=>M().translateSelf(1,2,0));
T('translateZ1back',()=>M().translateSelf(0,0,1).translateSelf(0,0,-1));
T('translateNaNz',()=>M().translateSelf(1,2,NaN));
T('translateNaN',()=>M().translateSelf(NaN,0,0));
T('translateAll0',()=>M().translate(0,0,0));
T('translate3dRO',()=>new DOMMatrixReadOnly().translate(0,0,5).translate(0,0,-5));
T('scaleSelfZ1',()=>M().scaleSelf(2,2,1));
T('scaleZ2back',()=>M().scaleSelf(1,1,2).scaleSelf(1,1,0.5));
T('scaleOriginZ',()=>M().scaleSelf(2,2,1,0,0,3));
T('scaleNaNz',()=>M().scaleSelf(2,2,NaN));
T('scale3dSelf1',()=>M().scale3dSelf(1));
T('scale3d2',()=>M().scale3d(2).scale3d(0.5));
T('scale3dOrigin',()=>M().scale3dSelf(1,0,0,4));
T('scaleNonUniform',()=>M().scaleNonUniform(2,3));
T('rotZ',()=>M().rotateSelf(90));
T('rotXY0',()=>M().rotateSelf(0,0,90));
T('rotX',()=>M().rotateSelf(360,0,0));
T('rotY',()=>M().rotateSelf(0,360,0));
T('rotYNaN',()=>M().rotateSelf(0,NaN,0));
T('rotAxisZ',()=>M().rotateAxisAngleSelf(0,0,1,90));
T('rotAxisX0',()=>M().rotateAxisAngleSelf(1,0,0,0));
T('rotAxisY',()=>M().rotateAxisAngleSelf(0,1,0,360));
T('rotAxisNoAxis',()=>M().rotateAxisAngleSelf(0,0,0,90));
T('rotAxisXz',()=>M().rotateAxisAngle(1,0,1,0));
out.rotAxisNaNflag=M().rotateAxisAngleSelf(NaN,0,1,90).is2D;
T('rotVec',()=>M().rotateFromVectorSelf(1,1));
T('skew',()=>M().skewXSelf(10).skewYSelf(10));
T('flip3d',()=>M().translateSelf(0,0,1).flipX());
T('flip2d',()=>M().flipY());
T('mul3d',()=>M().multiplySelf(new DOMMatrix([1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1])));
T('mul2d',()=>M().multiply(new DOMMatrix([2,0,0,2,0,0])));
T('mulDictFalse',()=>M().multiply({is2D:false}));
T('mulDict',()=>M().multiply({a:2}));
T('mulNone',()=>M().multiply());
T('premul3d',()=>M().preMultiplySelf({m33:1,is2D:false}));
T('mul3dBy2d',()=>new DOMMatrix([1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]).multiply(M()));
T('inv2d',()=>new DOMMatrix([2,0,0,2,1,1]).inverse());
T('inv2dSingular',()=>new DOMMatrix([0,0,0,0,1,1]).inverse());
T('inv3d',()=>new DOMMatrix('rotate3d(0,0,1,90deg)').inverse());
T('inv3dSingular',()=>new DOMMatrix([0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]).invertSelf());
T('set3dSame',()=>{const m=M();m.m13=0;return m});
T('set3d',()=>{const m=M();m.m13=1;m.m13=0;return m});
T('setM33',()=>{const m=M();m.m33=1;return m});
T('setM33b',()=>{const m=M();m.m33=2;m.m33=1;return m});
T('setM44NaN',()=>{const m=M();m.m44=NaN;return m});
T('setM13NaN',()=>{const m=M();m.m13=NaN;return m});
T('setM34neg0',()=>{const m=M();m.m34=-0;return m});
T('setA',()=>{const m=M();m.a=5;m.m41=3;return m});
T('setMatrixValue3d',()=>M().setMatrixValue('translateZ(0px)'));
T('setMatrixValue2d',()=>new DOMMatrix([1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]).setMatrixValue('translate(1px)'));
T('webkit',()=>new WebKitCSSMatrix('rotate3d(0,0,1,90deg)'));
T('clone3d',()=>structuredClone(new DOMMatrix('rotate3d(0,0,1,90deg)')));
T('clone2d',()=>structuredClone(new DOMMatrixReadOnly([1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]).translate(0,0,0)));
T('cloneRO3d',()=>structuredClone(new DOMMatrixReadOnly('scale3d(1,1,1)')));
T('clone2dflag',()=>structuredClone(new DOMMatrix([2,0,0,2,0,0])));
T('scaleZ2str',()=>new DOMMatrix('scaleZ(2)'));
T('scale3dZ2str',()=>new DOMMatrix('scale3d(1,1,2)'));
T('mixedScale3d',()=>new DOMMatrix('scale(2) scale3d(1,1,1) rotate(0deg) skew(1deg)'));
T('setM44ro',()=>{const m=new DOMMatrixReadOnly();try{m.m44=2}catch(e){}return m});
T('flipRO3d',()=>new DOMMatrixReadOnly('translateZ(0)').flipY().skewX(0).rotateFromVector(1,0).scaleNonUniform(2,3).rotate(0).translate(1,1));
T('fromMatrix3dFlagged',()=>DOMMatrix.fromMatrix(new DOMMatrix('translateZ(0)')));
T('mulMatrix3dFlagged',()=>M().multiply(new DOMMatrixReadOnly('translateZ(0)')));
T('ptMatrix3d',()=>{const p=new DOMPoint(1,2).matrixTransform({is2D:false});const m=M();m.e=p.x;return m});

for(const s of ['scaleZ(1)','scale3d(2,3,1)','scaleX(2)','translate3d(0,0,0)','rotateZ(0deg)','rotate3d(0,0,1,0deg)','rotateY(1deg) rotateY(-1deg)'])T('str '+s,()=>new DOMMatrix(s));
globalThis.out=out;"##;

const CHROME_156: &str = r##"{"none":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"empty":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"noneStr":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"rotate3dZ":[false,"matrix3d(0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"rotateZ":[true,"matrix(0, 1, -1, 0, 0, 0)",true,"DOMMatrix"],"rotateX0":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"rotateY0":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"translate3d0":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 0, 1)",false,"DOMMatrix"],"translateZ0":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"scale3d1":[false,"matrix3d(2, 0, 0, 0, 0, 3, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"scaleZ1":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"matrix3d2d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 0, 1)",false,"DOMMatrix"],"matrix2d":[true,"matrix(1, 0, 0, 1, 5, 6)",true,"DOMMatrix"],"perspective":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, -0.01, 0, 0, 0, 1)",false,"DOMMatrix"],"mixed":[false,"matrix3d(0.984807753012208, 0.17364817766693033, 0, 0, -0.17364817766693033, 0.984807753012208, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1)",false,"DOMMatrix"],"ro3dstr":[false,"matrix3d(0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrixReadOnly"],"seq6":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"seq16id":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"f32_16":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"f64_6":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrixReadOnly"],"dictNone":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"dictFalse":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"dictTrue":[true,"matrix(2, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"dict3d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"dictM33one":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"dictMinus0":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"dictNaN":[false,"InvalidStateError",false,"DOMMatrix"],"roDictFalse":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrixReadOnly"],"translateZ":[true,"matrix(1, 0, 0, 1, 1, 2)",true,"DOMMatrix"],"translateZ1back":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"translateNaNz":[false,"InvalidStateError",false,"DOMMatrix"],"translateNaN":[true,"InvalidStateError",true,"DOMMatrix"],"translateAll0":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"translate3dRO":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"scaleSelfZ1":[true,"matrix(2, 0, 0, 2, 0, 0)",true,"DOMMatrix"],"scaleZ2back":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"scaleOriginZ":[false,"matrix3d(2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"scaleNaNz":[false,"InvalidStateError",false,"DOMMatrix"],"scale3dSelf1":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"scale3d2":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"scale3dOrigin":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"scaleNonUniform":[true,"matrix(2, 0, 0, 3, 0, 0)",true,"DOMMatrix"],"rotZ":[true,"matrix(0, 1, -1, 0, 0, 0)",true,"DOMMatrix"],"rotXY0":[true,"matrix(0, 1, -1, 0, 0, 0)",true,"DOMMatrix"],"rotX":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"rotY":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"rotYNaN":[false,"InvalidStateError",false,"DOMMatrix"],"rotAxisZ":[true,"matrix(0, 1, -1, 0, 0, 0)",true,"DOMMatrix"],"rotAxisX0":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"rotAxisY":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"rotAxisNoAxis":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"rotAxisXz":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"rotAxisNaNflag":false,"rotVec":[true,"matrix(0.7071067811865476, 0.7071067811865476, -0.7071067811865476, 0.7071067811865476, 0, 0)",true,"DOMMatrix"],"skew":[true,"matrix(1.0310912041257634, 0.17632698070846498, 0.17632698070846498, 1, 0, 0)",true,"DOMMatrix"],"flip3d":[false,"matrix3d(-1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1)",false,"DOMMatrix"],"flip2d":[true,"matrix(1, 0, 0, -1, 0, 0)",true,"DOMMatrix"],"mul3d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"mul2d":[true,"matrix(2, 0, 0, 2, 0, 0)",true,"DOMMatrix"],"mulDictFalse":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"mulDict":[true,"matrix(2, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"mulNone":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"premul3d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"mul3dBy2d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"inv2d":[true,"matrix(0.5, 0, 0, 0.5, -0.5, -0.5)",true,"DOMMatrix"],"inv2dSingular":[false,"InvalidStateError",false,"DOMMatrix"],"inv3d":[false,"matrix3d(0, -1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"inv3dSingular":[false,"InvalidStateError",false,"DOMMatrix"],"set3dSame":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"set3d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"setM33":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"setM33b":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"setM44NaN":[false,"InvalidStateError",false,"DOMMatrix"],"setM13NaN":[false,"InvalidStateError",false,"DOMMatrix"],"setM34neg0":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"setA":[true,"matrix(5, 0, 0, 1, 3, 0)",true,"DOMMatrix"],"setMatrixValue3d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"setMatrixValue2d":[true,"matrix(1, 0, 0, 1, 1, 0)",true,"DOMMatrix"],"webkit":[false,"matrix3d(0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"clone3d":[false,"matrix3d(0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"clone2d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"cloneRO3d":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrixReadOnly"],"clone2dflag":[true,"matrix(2, 0, 0, 2, 0, 0)",true,"DOMMatrix"],"scaleZ2str":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"scale3dZ2str":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"mixedScale3d":[false,"matrix3d(2, 0, 0, 0, 0.03491012985643517, 2, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"setM44ro":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrixReadOnly"],"flipRO3d":[false,"matrix3d(2, 0, 0, 0, 0, -3, 0, 0, 0, 0, 1, 0, 2, -3, 0, 1)",false,"DOMMatrix"],"fromMatrix3dFlagged":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"mulMatrix3dFlagged":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"ptMatrix3d":[true,"matrix(1, 0, 0, 1, 1, 0)",true,"DOMMatrix"],"str scaleZ(1)":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"str scale3d(2,3,1)":[false,"matrix3d(2, 0, 0, 0, 0, 3, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"str scaleX(2)":[true,"matrix(2, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"str translate3d(0,0,0)":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"str rotateZ(0deg)":[true,"matrix(1, 0, 0, 1, 0, 0)",true,"DOMMatrix"],"str rotate3d(0,0,1,0deg)":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"],"str rotateY(1deg) rotateY(-1deg)":[false,"matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",false,"DOMMatrix"]}"##;

#[tokio::test(flavor = "current_thread")]
async fn dom_matrix_is2d_is_tracked_like_chrome156() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://matrix.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    let chrome: Value = serde_json::from_str(CHROME_156).unwrap();
    let mut mismatched = Vec::new();
    for (key, want) in chrome.as_object().unwrap() {
        if got.get(key) != Some(want) {
            mismatched.push(format!("{key}: got {:?}, Chrome {want}", got.get(key)));
        }
    }
    assert!(mismatched.is_empty(), "{}", mismatched.join("\n"));
    assert_eq!(got, chrome);
}
