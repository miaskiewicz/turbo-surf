//! DOMQuad (turbo-surf#69): construction from DOMPointInit dictionaries (lexical
//! member order, WebIDL conversion errors), `fromRect`/`fromQuad`, live `p1`..`p4`
//! DOMPoints, `getBounds()`, `toJSON()`, Web IDL shape and brand checks, structured
//! clone / MessagePort, and members that keep working after the page replaces the
//! intrinsics they would otherwise look up. Expected values: Chrome 156
//! (156.0.8078.47, headless) running the same script on a localhost page.
use serde_json::Value;
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const out={};const T=(k,f)=>{try{out[k]=f()}catch(e){out[k]=e.name+':'+e.message}};
const s=v=>Array.isArray(v)?v.map(s):typeof v==='number'?String(v):v;
const pj=p=>p&&[p.constructor.name,s(p.x),s(p.y),s(p.z),s(p.w)];
const ps=q=>[1,2,3,4].map(i=>pj(q['p'+i]));
T('basic',()=>{const q=new DOMQuad(new DOMPoint(1,1));return [q.p1.x,q.p2.w,q.getBounds().width]});
T('fromRect',()=>ps(DOMQuad.fromRect({x:1,y:2,width:3,height:4})));
T('fromRectNone',()=>ps(DOMQuad.fromRect()));
T('fromRectNeg',()=>ps(DOMQuad.fromRect({width:-3,height:-4})));
T('fromRectNaN',()=>ps(DOMQuad.fromRect({width:NaN})));
T('fromRectDOMRect',()=>ps(DOMQuad.fromRect(new DOMRect(1,2,3,4))));
T('fromQuad',()=>ps(DOMQuad.fromQuad({p1:{x:1,y:2},p3:{x:5,y:6,z:7,w:8}})));
T('fromQuadNone',()=>ps(DOMQuad.fromQuad()));
T('ctorDicts',()=>ps(new DOMQuad({x:1},{y:2},{z:3},{w:4})));
T('ctorNull',()=>ps(new DOMQuad(null,undefined)));
T('ctorNone',()=>{const q=new DOMQuad();return [pj(q.p1),q.p1===q.p1,q.p1 instanceof DOMPoint,Object.getPrototypeOf(q.p1)===DOMPoint.prototype]});
T('copy',()=>{const p=new DOMPoint(1,2);const q=new DOMQuad(p);q.p1.x=9;return [q.p1===p,p.x,q.p1.x]});
T('live',()=>{const q=new DOMQuad();q.p1.x=5;q.p3.y=-2;const b=q.getBounds();return [b.x,b.y,b.width,b.height]});
T('bounds',()=>{const b=new DOMQuad({x:1,y:5},{x:-2,y:3},{x:7,y:-1},{x:0,y:0}).getBounds();return [b.constructor.name,b.x,b.y,b.width,b.height,b.getBounds===undefined]});
T('boundsNaN',()=>[[{x:NaN},{x:1}],[{x:1},{x:NaN}],[{x:1},{x:2},{x:3},{x:NaN}]].map(a=>{const b=new DOMQuad(...a).getBounds();return s([b.x,b.y,b.width,b.height])}));
T('boundsInf',()=>{const b=new DOMQuad({x:Infinity},{x:-Infinity}).getBounds();return s([b.x,b.width,b.left,b.right])});
T('boundsFresh',()=>{const q=new DOMQuad();return q.getBounds()===q.getBounds()});
T('json',()=>JSON.stringify(new DOMQuad({x:1},{y:-0})));
T('toJSON',()=>{const j=new DOMQuad().toJSON();return [Object.keys(j),Object.getPrototypeOf(j)===Object.prototype,j.p1.constructor.name,j.p1===new DOMQuad().p1]});
T('toJSONsame',()=>{const q=new DOMQuad();return q.toJSON().p2===q.p2});
T('shape',()=>[DOMQuad.length,DOMQuad.name,DOMQuad.fromRect.length,DOMQuad.fromQuad.length,Object.getOwnPropertyNames(DOMQuad.prototype),Object.getOwnPropertyNames(DOMQuad),DOMQuad.prototype.getBounds.length,DOMQuad.prototype.toJSON.length,DOMQuad.prototype[Symbol.toStringTag],Object.prototype.toString.call(new DOMQuad()),Object.keys(new DOMQuad()),Object.getPrototypeOf(DOMQuad.prototype)===Object.prototype,Object.getPrototypeOf(DOMQuad)===Function.prototype]);
T('desc',()=>{const d=Object.getOwnPropertyDescriptor(DOMQuad.prototype,'p1');return [typeof d.get,d.set,d.enumerable,d.configurable,d.get.name,d.get.length]});
T('methodDesc',()=>{const d=Object.getOwnPropertyDescriptor(DOMQuad.prototype,'getBounds');const f=Object.getOwnPropertyDescriptor(DOMQuad,'fromRect');return [d.writable,d.enumerable,d.configurable,f.writable,f.enumerable,f.configurable]});
T('native',()=>[Function.prototype.toString.call(DOMQuad),Function.prototype.toString.call(DOMQuad.fromRect),Function.prototype.toString.call(DOMQuad.prototype.getBounds),Function.prototype.toString.call(Object.getOwnPropertyDescriptor(DOMQuad.prototype,'p4').get),'prototype' in DOMQuad.prototype.getBounds]);
T('callNoNew',()=>DOMQuad());
T('brandGetter',()=>Object.getOwnPropertyDescriptor(DOMQuad.prototype,'p1').get.call({}));
T('brandProto',()=>Object.getOwnPropertyDescriptor(DOMQuad.prototype,'p2').get.call(Object.create(DOMQuad.prototype)));
T('brandBounds',()=>DOMQuad.prototype.getBounds.call(new DOMPoint()));
T('brandJSON',()=>DOMQuad.prototype.toJSON.call(DOMQuad.prototype));
T('badArg',()=>new DOMQuad(5));
T('badArgLater',()=>new DOMQuad({},'x'));
T('bigint',()=>new DOMQuad({x:1n}));
T('symbol',()=>new DOMQuad({y:Symbol()}));
T('rectBad',()=>DOMQuad.fromRect(5));
T('rectBigint',()=>DOMQuad.fromRect({height:1n}));
T('quadBad',()=>DOMQuad.fromQuad(5));
T('quadBadPoint',()=>DOMQuad.fromQuad({p2:5}));
T('quadBigint',()=>DOMQuad.fromQuad({p4:{w:1n}}));
T('pointOrder',()=>{const log=[];const g=n=>({get w(){log.push(n+'w');return 1},get x(){log.push(n+'x');return {valueOf(){log.push(n+'xv');return 0}}},get y(){log.push(n+'y')},get z(){log.push(n+'z')}});new DOMQuad(g(1),g(2));return log});
T('rectOrder',()=>{const log=[];DOMQuad.fromRect({get x(){log.push('x');return 0},get y(){log.push('y');return 0},get width(){log.push('w');return 0},get height(){log.push('h');return 0}});return log});
T('quadOrder',()=>{const log=[];DOMQuad.fromQuad({get p4(){log.push('p4')},get p1(){log.push('p1');return {get x(){log.push('p1x')}}},get p2(){log.push('p2')},get p3(){log.push('p3')}});return log});
T('subclass',()=>{class Q extends DOMQuad{};const q=new Q({x:2});return [q instanceof Q,q.p1.x,Q.fromRect() instanceof Q,DOMQuad.fromRect.call(Q) instanceof Q]});
T('detached',()=>{const f=DOMQuad.fromRect;return f({width:2}).p2.x});
T('assign',()=>{const q=new DOMQuad();q.p1=5;return Object.prototype.toString.call(q.p1)});
T('assignStrict',()=>{'use strict';const q=new DOMQuad();q.p1=5;return 1});
T('hostile',()=>{const saved=[WeakMap.prototype.get,WeakMap.prototype.set,Math.min,Math.max,window.DOMRect,window.DOMPoint,Array.prototype.map,Array.from,window.Number];let raw;
  WeakMap.prototype.get=()=>{throw 1};WeakMap.prototype.set=()=>{throw 2};Math.min=()=>42;Math.max=()=>43;window.DOMRect=function(){return {fake:1}};window.DOMPoint=function(){return {fake:2}};Array.prototype.map=()=>{throw 3};Array.from=()=>{throw 4};window.Number=()=>44;
  try{const q=new DOMQuad({x:3},{y:4});const r=DOMQuad.fromRect({width:2,height:3});const b=q.getBounds();raw=[q.p1.x,q.p2.y,r.p3,b,JSON.stringify(q.toJSON().p2),DOMQuad.fromQuad({p1:{x:7}})]}
  finally{[WeakMap.prototype.get,WeakMap.prototype.set,Math.min,Math.max,window.DOMRect,window.DOMPoint,Array.prototype.map,Array.from,window.Number]=saved}
  const b=raw[3];return [raw[0],raw[1],pj(raw[2]),b.constructor.name,b.x,b.y,b.width,b.height,raw[4],ps(raw[5])]});
T('clone',()=>{const src=new DOMQuad({x:1,y:1},{x:2},{z:3},{w:4});const c=structuredClone(src);src.p1.x=99;return [c.constructor.name,ps(c),c.p1===c.p1,c instanceof DOMQuad,Object.keys(c)]});
T('cloneAlias',()=>{const q=new DOMQuad();const [a,b]=structuredClone([q,q]);return [a===b,a.p1===b.p1]});
T('cloneSub',()=>{class Q extends DOMQuad{};const c=structuredClone(new Q({x:5}));return [c.constructor.name,c.p1.x]});
T('cloneFake',()=>{const c=structuredClone(Object.create(DOMQuad.prototype));return Object.prototype.toString.call(c)});
const {port1,port2}=new MessageChannel();
port2.onmessage=e=>{const q=e.data;out.port=[q.constructor.name,ps(q),q.getBounds().width];};
port1.postMessage(new DOMQuad({x:1},{x:4,y:2}));
globalThis.out=out;"#;

const CHROME_156: &str = r#"{"basic":[1,1,1],"fromRect":[["DOMPoint","1","2","0","1"],["DOMPoint","4","2","0","1"],["DOMPoint","4","6","0","1"],["DOMPoint","1","6","0","1"]],"fromRectNone":[["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"]],"fromRectNeg":[["DOMPoint","0","0","0","1"],["DOMPoint","-3","0","0","1"],["DOMPoint","-3","-4","0","1"],["DOMPoint","0","-4","0","1"]],"fromRectNaN":[["DOMPoint","0","0","0","1"],["DOMPoint","NaN","0","0","1"],["DOMPoint","NaN","0","0","1"],["DOMPoint","0","0","0","1"]],"fromRectDOMRect":[["DOMPoint","1","2","0","1"],["DOMPoint","4","2","0","1"],["DOMPoint","4","6","0","1"],["DOMPoint","1","6","0","1"]],"fromQuad":[["DOMPoint","1","2","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","5","6","7","8"],["DOMPoint","0","0","0","1"]],"fromQuadNone":[["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"]],"ctorDicts":[["DOMPoint","1","0","0","1"],["DOMPoint","0","2","0","1"],["DOMPoint","0","0","3","1"],["DOMPoint","0","0","0","4"]],"ctorNull":[["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"]],"ctorNone":[["DOMPoint","0","0","0","1"],true,true,true],"copy":[false,1,9],"live":[0,-2,5,2],"bounds":["DOMRect",-2,-1,9,6,true],"boundsNaN":[["NaN","0","NaN","0"],["NaN","0","NaN","0"],["NaN","0","NaN","0"]],"boundsInf":["-Infinity","Infinity","NaN","NaN"],"boundsFresh":false,"json":"{\"p1\":{\"x\":1,\"y\":0,\"z\":0,\"w\":1},\"p2\":{\"x\":0,\"y\":0,\"z\":0,\"w\":1},\"p3\":{\"x\":0,\"y\":0,\"z\":0,\"w\":1},\"p4\":{\"x\":0,\"y\":0,\"z\":0,\"w\":1}}","toJSON":[["p1","p2","p3","p4"],true,"DOMPoint",false],"toJSONsame":true,"shape":[0,"DOMQuad",0,0,["p1","p2","p3","p4","getBounds","toJSON","constructor"],["length","name","prototype","fromQuad","fromRect"],0,0,"DOMQuad","[object DOMQuad]",[],true,true],"desc":["function",null,true,true,"get p1",0],"methodDesc":[true,true,true,true,true,true],"native":["function DOMQuad() { [native code] }","function fromRect() { [native code] }","function getBounds() { [native code] }","function get p4() { [native code] }",false],"callNoNew":"TypeError:Failed to construct 'DOMQuad': Please use the 'new' operator, this DOM object constructor cannot be called as a function.","brandGetter":"TypeError:Illegal invocation","brandProto":"TypeError:Illegal invocation","brandBounds":"TypeError:Illegal invocation","brandJSON":"TypeError:Illegal invocation","badArg":"TypeError:Failed to construct 'DOMQuad': The provided value is not of type 'DOMPointInit'.","badArgLater":"TypeError:Failed to construct 'DOMQuad': The provided value is not of type 'DOMPointInit'.","bigint":"TypeError:Failed to construct 'DOMQuad': Failed to read the 'x' property from 'DOMPointInit': Cannot convert a BigInt value to a number","symbol":"TypeError:Failed to construct 'DOMQuad': Failed to read the 'y' property from 'DOMPointInit': Cannot convert a Symbol value to a number","rectBad":"TypeError:Failed to execute 'fromRect' on 'DOMQuad': The provided value is not of type 'DOMRectInit'.","rectBigint":"TypeError:Failed to execute 'fromRect' on 'DOMQuad': Failed to read the 'height' property from 'DOMRectInit': Cannot convert a BigInt value to a number","quadBad":"TypeError:Failed to execute 'fromQuad' on 'DOMQuad': The provided value is not of type 'DOMQuadInit'.","quadBadPoint":"TypeError:Failed to execute 'fromQuad' on 'DOMQuad': Failed to read the 'p2' property from 'DOMQuadInit': The provided value is not of type 'DOMPointInit'.","quadBigint":"TypeError:Failed to execute 'fromQuad' on 'DOMQuad': Failed to read the 'p4' property from 'DOMQuadInit': Failed to read the 'w' property from 'DOMPointInit': Cannot convert a BigInt value to a number","pointOrder":["1w","1x","1xv","1y","1z","2w","2x","2xv","2y","2z"],"rectOrder":["h","w","x","y"],"quadOrder":["p1","p1x","p2","p3","p4"],"subclass":[true,2,false,false],"detached":2,"assign":"[object DOMPoint]","assignStrict":"TypeError:Cannot set property p1 of #<DOMQuad> which has only a getter","hostile":[3,4,["DOMPoint","2","3","0","1"],"DOMRect",0,0,3,4,"{\"x\":0,\"y\":4,\"z\":0,\"w\":1}",[["DOMPoint","7","0","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"]]],"clone":["DOMQuad",[["DOMPoint","1","1","0","1"],["DOMPoint","2","0","0","1"],["DOMPoint","0","0","3","1"],["DOMPoint","0","0","0","4"]],true,true,[]],"cloneAlias":[true,true],"cloneSub":["DOMQuad",5],"cloneFake":"[object Object]","port":["DOMQuad",[["DOMPoint","1","0","0","1"],["DOMPoint","4","2","0","1"],["DOMPoint","0","0","0","1"],["DOMPoint","0","0","0","1"]],4]}"#;

#[tokio::test(flavor = "current_thread")]
async fn dom_quad_matches_chrome156() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://quad.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_engine("__tsNs.runTimers(200)").await.unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    let chrome: Value = serde_json::from_str(CHROME_156).unwrap();
    for (key, want) in chrome.as_object().unwrap() {
        assert_eq!(got.get(key), Some(want), "{key}");
    }
    assert_eq!(got, chrome);
}
