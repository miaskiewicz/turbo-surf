//! DOMPoint/DOMMatrix members keep working after the page replaces the intrinsics
//! their implementation would otherwise look up at use time (turbo-surf#70):
//! WeakMap/WeakSet methods, `Number`, `TypeError`, Array/Object/Math helpers and
//! `Reflect.apply`, and the geometry methods/accessors one member would
//! otherwise call on another through the prototype. Chrome's platform objects never consult them, so the same
//! script yields identical values before and while they are patched. Expected
//! values: Chrome 156 (156.0.8078.47, headless) running the script on a localhost page.
use serde_json::Value;
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const out={};const xGet=Object.getOwnPropertyDescriptor(DOMPointReadOnly.prototype,'x').get;
const names=[[WeakMap.prototype,'get'],[WeakMap.prototype,'set'],[WeakMap.prototype,'has'],[WeakSet.prototype,'has'],[WeakSet.prototype,'add'],[window,'Number'],[window,'TypeError'],[Array,'from'],[Array.prototype,'map'],[Array.prototype,'every'],[Array.prototype,'some'],[Array.prototype,'join'],[Object,'entries'],[Math,'hypot'],[Math,'cos'],[Math,'sin'],[Math,'tan'],[Math,'atan2'],[Math,'abs'],[Math,'fround'],[Reflect,'apply'],[Object,'create']];
const run=()=>{
  const p=new DOMPoint(1,'2');p.x=7;p.z='3';
  const r=new DOMPointReadOnly(5,6);
  const m=new DOMMatrix([1,2,3,4,5,6]);m.a=9;m.m42=8;
  const t=m.translate(1,1).scale(2).rotate(0).skewX(45);
  const t3=new DOMMatrix([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]);
  const e=(f)=>{try{return f()}catch(x){return 'threw:'+(x&&x.constructor&&x.constructor.name)}};
  return [p.x,p.y,p.z,p.w,JSON.stringify(p),JSON.stringify(r),m.a,m.b,m.f,m.m42,m.is2D,m.isIdentity,new DOMMatrix().isIdentity,
    m.toString(),t.toString(),m.inverse().a,m.multiply(m).e,DOMMatrix.fromMatrix({a:2,e:3}).toString(),DOMPoint.fromPoint({x:4}).x,
    m.transformPoint({x:1,y:1}).y,new DOMMatrix('translate(3px, 4px) rotate(30deg)').e,t3.inverse().m11,new DOMMatrix(new Float64Array([1,0,0,1,2,3])).f,
    Array.prototype.slice.call(m.toFloat64Array()).length,(j=>{const k=Object.keys(j).sort();let o='';for(let i=0;i<k.length;i++)o+=k[i]+'='+j[k[i]]+';';return o})(new DOMMatrixReadOnly([1,0,0,1,2,3]).toJSON()),new DOMMatrixReadOnly([1,0,0,1,2,3]).toString(),new DOMMatrixReadOnly([2,0,0,2,0,0]).scaleNonUniform(2,3).toString(),new DOMMatrixReadOnly().rotateFromVector(-1,0).a,new DOMPointReadOnly(1,2).matrixTransform({e:1}).x,
    e(()=>new DOMPoint(1n)),e(()=>new DOMMatrix([1,2])),e(()=>xGet.call({})),
    p.matrixTransform(m).x,m.rotateAxisAngle(0,0,1,90).b,m.flipX().a,new DOMMatrix().scaleSelf(2,3,1,1,1).e];
};
const want=JSON.stringify(run());
const methods=[[DOMMatrix.prototype,'translateSelf'],[DOMMatrix.prototype,'rotateSelf'],[DOMMatrix.prototype,'rotateAxisAngleSelf'],[DOMMatrix.prototype,'skewXSelf'],[DOMMatrix.prototype,'skewYSelf'],[DOMMatrix.prototype,'multiplySelf'],[DOMMatrix.prototype,'invertSelf'],[DOMMatrixReadOnly,'fromMatrix'],[DOMPointReadOnly,'fromPoint']];
const accessors=['a','b','c','d','e','f','m11','m44'].map(k=>[DOMMatrixReadOnly.prototype,k]).concat(['x','y','z','w'].map(k=>[DOMPointReadOnly.prototype,k]));
const all=names.concat(methods);
const saved=all.map(([o,k])=>o[k]);
const savedAccessors=accessors.map(([o,k])=>Object.getOwnPropertyDescriptor(o,k));
let got;
for(const [o,k] of all) o[k]=function(){throw new Error('patched '+k)};
for(const [o,k] of accessors) Object.defineProperty(o,k,{get(){throw new Error('patched '+k)},configurable:true});
try{got=JSON.stringify(run())}catch(x){got='threw '+String(x&&x.message)}
finally{all.forEach(([o,k],i)=>{o[k]=saved[i]});accessors.forEach(([o,k],i)=>Object.defineProperty(o,k,savedAccessors[i]))}
out.same=got===want;out.got=JSON.parse(want);out.hostile=got===want?'same':got;
globalThis.out=out;"#;

const CHROME_156: &str = r#"{"same":true,"got":[7,2,3,1,"{\"x\":7,\"y\":2,\"z\":3,\"w\":1}","{\"x\":5,\"y\":6,\"z\":0,\"w\":1}",9,2,8,8,true,false,true,"matrix(9, 2, 3, 4, 5, 8)","matrix(18, 4, 23.999999999999996, 12, 17, 14)",0.13333333333333333,74,"matrix(2, 0, 0, 1, 3, 0)",4,14,3,null,3,16,"a=1;b=0;c=0;d=1;e=2;f=3;is2D=true;isIdentity=false;m11=1;m12=0;m13=0;m14=0;m21=0;m22=1;m23=0;m24=0;m31=0;m32=0;m33=1;m34=0;m41=2;m42=3;m43=0;m44=1;","matrix(1, 0, 0, 1, 2, 3)","matrix(4, 0, 0, 6, 0, 0)",-1,2,"threw:TypeError","threw:TypeError","threw:TypeError",74,4,-9,-1],"hostile":"same"}"#;

#[tokio::test(flavor = "current_thread")]
async fn geometry_members_ignore_page_patched_intrinsics_like_chrome156() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    let chrome: Value = serde_json::from_str(CHROME_156).unwrap();
    assert_eq!(got["hostile"], chrome["hostile"]);
    assert_eq!(got, chrome);
}
