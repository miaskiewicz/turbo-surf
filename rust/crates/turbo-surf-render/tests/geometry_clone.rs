//! Structured clone of the serializable Geometry interfaces (turbo-surf#66):
//! DOMPoint, DOMPointReadOnly, DOMMatrix and DOMMatrixReadOnly arrive with their
//! interface and values through structuredClone and MessagePort.postMessage, the
//! interface comes from the platform object (a subclass clones as its base, a
//! prototype-only object as a plain object), aliases are preserved and the copy is a
//! snapshot (DOMQuad clones too: dom_quad.rs, turbo-surf#69). Expected log: Chrome 156 (156.0.8078.47, headless) running the same
//! script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const log=[];
const d=v=>{const n=v&&v.constructor&&v.constructor.name;
 if(n==='DOMPoint'||n==='DOMPointReadOnly')return [n,v.x,v.y,v.z,v.w,Object.is(v.y,-0)];
 if(n==='DOMRect'||n==='DOMRectReadOnly')return [n,v.x,v.y,v.width,v.height];
 if(n==='DOMMatrix'||n==='DOMMatrixReadOnly')return [n,v.is2D,Array.from(v.toFloat64Array())];
 return [n,JSON.stringify(v)];};
const values=()=>[new DOMPoint(1,2,3,4),new DOMPointReadOnly(5,6,7,8),new DOMRectReadOnly(1,2,3,4),new DOMMatrix([1,2,3,4,5,6]),new DOMMatrixReadOnly([1,0,0,1,7,8]),new DOMMatrix([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]),new DOMPoint(NaN,-0,Infinity),DOMPoint.fromPoint({x:2}),DOMMatrixReadOnly.fromMatrix({e:3}),new DOMMatrix().translate(4,5),new DOMPointReadOnly(1,2).matrixTransform(new DOMMatrix([2,0,0,2,1,1]))];
log.push(['clone',structuredClone(values()).map(d)]);
const p=new DOMPoint(9,8);const pair=structuredClone([p,p]);log.push(['alias',pair[0]===pair[1],pair[0] instanceof DOMPoint,Object.prototype.toString.call(pair[0])]);
const m=structuredClone(new DOMMatrix());m.a=3;log.push(['mutable',m.a,m.isIdentity,structuredClone(new DOMMatrixReadOnly()).isIdentity]);
const r=structuredClone(new DOMPointReadOnly(1,2));try{r.x=5}catch(e){};log.push(['readonly',r.x,'x' in r,Object.keys(r).length]);
const sub=new (class extends DOMPoint{})(1,2);log.push(['subclass',d(structuredClone(sub))]);
const ro=new (class extends DOMMatrixReadOnly{})([1,2,3,4,5,6]);log.push(['subclass matrix',d(structuredClone(ro))]);
const o=new DOMPoint(1,2);o.extra=1;log.push(['expando',structuredClone(o).extra]);
const src=new DOMPoint(1,2);const copy=structuredClone(src);src.x=10;log.push(['snapshot',copy.x]);
log.push(['fake',d(structuredClone(Object.create(DOMPoint.prototype,{a:{value:1,enumerable:true}})))]);
const {port1,port2}=new MessageChannel();
port2.onmessage=e=>{log.push(['port',e.data.map(d)]);};
port1.postMessage(values());
globalThis.log=log;"#;

const CHROME_156: &str = r#"[["clone",[["DOMPoint",1,2,3,4,false],["DOMPointReadOnly",5,6,7,8,false],["DOMRectReadOnly",1,2,3,4],["DOMMatrix",true,[1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1]],["DOMMatrixReadOnly",true,[1,0,0,0,0,1,0,0,0,0,1,0,7,8,0,1]],["DOMMatrix",false,[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]],["DOMPoint",null,0,null,1,true],["DOMPoint",2,0,0,1,false],["DOMMatrixReadOnly",true,[1,0,0,0,0,1,0,0,0,0,1,0,3,0,0,1]],["DOMMatrix",true,[1,0,0,0,0,1,0,0,0,0,1,0,4,5,0,1]],["DOMPoint",3,5,0,1,false]]],["alias",true,true,"[object DOMPoint]"],["mutable",3,false,true],["readonly",1,true,0],["subclass",["DOMPoint",1,2,0,1,false]],["subclass matrix",["DOMMatrixReadOnly",true,[1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1]]],["expando",null],["snapshot",1],["fake",["Object","{\"a\":1}"]],["port",[["DOMPoint",1,2,3,4,false],["DOMPointReadOnly",5,6,7,8,false],["DOMRectReadOnly",1,2,3,4],["DOMMatrix",true,[1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1]],["DOMMatrixReadOnly",true,[1,0,0,0,0,1,0,0,0,0,1,0,7,8,0,1]],["DOMMatrix",false,[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]],["DOMPoint",null,0,null,1,true],["DOMPoint",2,0,0,1,false],["DOMMatrixReadOnly",true,[1,0,0,0,0,1,0,0,0,0,1,0,3,0,0,1]],["DOMMatrix",true,[1,0,0,0,0,1,0,0,0,0,1,0,4,5,0,1]],["DOMPoint",3,5,0,1,false]]]]"#;

#[tokio::test(flavor = "current_thread")]
async fn geometry_interfaces_survive_structured_clone_like_chrome() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_engine("__tsNs.runTimers(200)").await.unwrap();
    let log: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression("log").await.unwrap()).unwrap();
    let chrome: serde_json::Value = serde_json::from_str(CHROME_156).unwrap();
    for (got, want) in log
        .as_array()
        .unwrap()
        .iter()
        .zip(chrome.as_array().unwrap())
    {
        assert_eq!(got, want);
    }
    assert_eq!(log, chrome);
}

/// A child realm's point/matrix clones into this realm's interface, and the clone
/// hooks keep working after the page replaces the WeakMap/WeakSet/Reflect/Object/
/// Array/Number intrinsics they would otherwise call (restored afterwards so the
/// engine's own lifecycle scripts keep running).
const HOSTILE: &str = r#"const log=[];const f=document.createElement('iframe');document.body.append(f);
const W=f.contentWindow;const foreign=new W.DOMMatrix([1,2,3,4,5,6]);
const fc=structuredClone([foreign,new W.DOMPointReadOnly(1,2)]);
log.push(['foreign',fc[0].constructor===DOMMatrix,fc[0].b,fc[1].constructor===DOMPointReadOnly,fc[1].y]);
const keep=new DOMPoint(3,4);
const saved=[WeakMap.prototype.get,WeakMap.prototype.set,WeakSet.prototype.has,WeakSet.prototype.add,Reflect.apply,Object.create,Array.prototype[Symbol.iterator],Number];
WeakMap.prototype.get=()=>{throw 1};WeakMap.prototype.set=()=>{throw 2};WeakSet.prototype.has=()=>{throw 3};WeakSet.prototype.add=()=>{throw 4};
Reflect.apply=()=>{throw 5};Object.create=()=>{throw 6};Array.prototype[Symbol.iterator]=function*(){throw 7};Number=()=>{throw 8};
let r,error;try{r=structuredClone(keep)}catch(e){error=e}
Array.prototype[Symbol.iterator]=saved[6];WeakMap.prototype.get=saved[0];WeakMap.prototype.set=saved[1];WeakSet.prototype.has=saved[2];WeakSet.prototype.add=saved[3];Reflect.apply=saved[4];Object.create=saved[5];Number=saved[7];
log.push(error===undefined?['patched',r.constructor.name,r.x,r.y]:['patched','throw',String(error)]);
globalThis.log=log;"#;

const HOSTILE_CHROME_156: &str = r#"[["foreign",true,2,true,2],["patched","DOMPoint",3,4]]"#;

#[tokio::test(flavor = "current_thread")]
async fn foreign_realm_and_patched_intrinsics_clone_like_chrome() {
    let html = format!("<!doctype html><body><script>{HOSTILE}</script></body>");
    let mut page = PageSession::open(&html, "https://geometry.test/", "", "", 5000)
        .await
        .unwrap();
    let log = page.eval_json_expression("log").await.unwrap();
    assert_eq!(log, HOSTILE_CHROME_156);
}
