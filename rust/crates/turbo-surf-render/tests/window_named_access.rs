//! HTML "named access on the Window object" (turbo-surf#77): the WindowProperties
//! object between Window.prototype and EventTarget.prototype exposes elements by
//! id (any namespace), embed/form/img/object elements by name (HTML namespace),
//! and child browsing contexts by their current name (`window.name`), ahead of elements. One match
//! is the element, several are one live HTMLCollection. Own globals, `var`/function
//! declarations and every property on the prototype chain shadow named items;
//! template contents, shadow trees and disconnected elements are not exposed; the
//! object rejects sets, defines, deletes and prototype changes like Blink's
//! interceptor. Expected values: Chrome 156 (156.0.8078.47, headless) running the
//! same document on a localhost page.
//!
//! Named access is enabled only after the trusted bootstrap, and never resolves
//! the host result channel (`__RESULT`), so page ids cannot redirect it.
use serde_json::Value;
use turbo_surf_render::PageSession;

const BODY: &str = r#"<div id=f>F</div><div id=dup></div><span id=dup></span>
<img name=imgn><form name=formn></form><embed name=embn><object name=objn></object>
<div name=divn></div><input name=inputn><a name=an></a><iframe name=ifn></iframe><iframe id=ifid></iframe>
<img name=both id=both2><img id=imgid name=imgidname><img name=imgmix><div id=imgmix></div><img id=shared name=x1><img name=shared>
<div id=addEventListener></div><div id=toString></div><div id=document></div><div id=location></div><div id=alert></div><div id=hasOwnProperty></div><div id=name></div><div id=Window></div><div id=onload></div><div id=frames></div><div id=constructor></div><div id=__proto__></div><div id=valueOf></div>
<div id=0></div><div id=""></div><svg><circle id=svgc></circle><image name=svgimg></image></svg><p id=decl></p><p id=letdecl></p><p id=fndecl></p><p id=F2></p>
<template><div id=intemplate></div></template><div id=host></div>
<script>var decl; let letdecl=5; function fndecl(){}</script>"#;

const SCRIPT: &str = r#"const out={};const T=(k,f)=>{try{out[k]=f()}catch(e){out[k]=e.name+':'+e.message}};
const d=v=>v===undefined?'undefined':v===null?'null':v===window?'window':(v&&v.window===v)?'windowproxy':(Object.prototype.toString.call(v)==='[object HTMLCollection]')?['HTMLCollection',v.length,...Array.from(v,x=>x.localName)]:(v&&v.nodeType===1)?v.localName+'#'+(v.id||v.getAttribute('name')):typeof v;
const wp=Object.getPrototypeOf(Window.prototype);
T('id',()=>[d(window.f),d(f),window.f===document.getElementById('f'),d(globalThis.f),d(self.f),d(F2),d(window.F)]);
T('dup',()=>[d(window.dup),window.dup===window.dup,typeof dup]);
T('names',()=>['imgn','formn','embn','objn','divn','inputn','an','ifid','svgimg'].map(n=>[n,d(window[n])]));
T('frameName',()=>{const w=document.querySelector('iframe[name=ifn]').contentWindow;w.setAttribute('name','ifn');return [d(window.ifn),window.ifn===w]});
T('both',()=>[d(window.both),d(window.both2),d(window.imgid),d(window.imgidname),d(window.imgmix),d(window.shared)]);
T('builtins',()=>['addEventListener','toString','document','location','alert','hasOwnProperty','name','Window','onload','frames','constructor','valueOf'].map(n=>[n,d(window[n])]));
T('proto',()=>Object.prototype.toString.call(window.__proto__));
T('index',()=>[d(window[0]),d(window['']),d(window.svgc)]);
T('decl',()=>[d(window.decl),d(window.letdecl),d(letdecl),typeof window.fndecl]);
T('scopes',()=>{const h=document.getElementById('host').attachShadow({mode:'open'});h.innerHTML='<div id=inshadow></div>';return [d(window.intemplate),d(window.inshadow)]});
T('live',()=>{const e=document.createElement('b');e.id='later';const r1=d(window.later);document.body.append(e);const r2=d(window.later);e.id='later2';const r3=[d(window.later),d(window.later2)];e.setAttribute('id','later3');const r4=d(window.later3);e.remove();return [r1,r2,r3,r4,d(window.later3)]});
T('liveName',()=>{const e=document.createElement('img');const r0=d(window.liven);document.body.append(e);e.setAttribute('name','liven');const r1=d(window.liven);e.removeAttribute('name');return [r0,r1,d(window.liven)]});
T('detached',()=>{const e=document.createElement('img');e.setAttribute('name','detimg');e.id='detid';return [d(window.detimg),d(window.detid)]});
T('collection',()=>{const a=document.createElement('i');a.id='cs';const b=document.createElement('i');b.id='cs';document.body.append(a,b);const c=window.cs;const n1=c.length;const extra=document.createElement('img');extra.setAttribute('name','cs');document.body.append(extra);const n2=c.length;extra.remove();b.remove();const single=d(window.cs);document.body.append(b);return [n1,n2,c.length,single,window.cs===c,c.namedItem('cs')===a,c[1]===b,Object.prototype.toString.call(c)]});
T('bare',()=>{const e=document.createElement('u');e.id='bare1';document.body.append(e);return [d(bare1),typeof bare1]});
T('in',()=>['f' in window,'nope' in window,Object.hasOwn(window,'f'),'f' in Window.prototype,'f' in wp,'toString' in wp]);
T('wpShape',()=>[Object.prototype.toString.call(wp),Object.getOwnPropertyNames(wp),Reflect.ownKeys(wp).map(String),Object.getPrototypeOf(wp)===EventTarget.prototype,typeof globalThis.WindowProperties,wp.constructor===EventTarget]);
T('wpDescriptor',()=>{const x=Object.getOwnPropertyDescriptor(wp,'f');return [x.writable,x.enumerable,x.configurable,x.value===document.getElementById('f'),Object.getOwnPropertyDescriptor(wp,'nope')===undefined,Object.getOwnPropertyDescriptor(wp,'toString')===undefined,Object.hasOwn(wp,'f'),Object.hasOwn(wp,'toString')]});
T('wpGetters',()=>[d(Reflect.get(wp,'f',{})),d(Object.create(wp).f),d(wp[Symbol.iterator])]);
T('wpSloppySet',()=>{wp.zz=1;wp.f=2;return [Object.hasOwn(wp,'zz'),d(wp.zz),d(wp.f)]});
T('wpStrictSet',()=>{'use strict';wp.zz=1;return 'no throw'});
T('wpDefine',()=>[Reflect.defineProperty(wp,'yy',{value:1,configurable:true}),Object.hasOwn(wp,'yy'),Reflect.defineProperty(wp,'f',{value:1}),d(wp.f),Reflect.ownKeys(wp).length]);
T('wpDelete',()=>[Reflect.deleteProperty(wp,'f'),d(wp.f),Reflect.deleteProperty(wp,'nope')]);
T('wpProto',()=>[Reflect.setPrototypeOf(wp,null),Reflect.setPrototypeOf(wp,EventTarget.prototype),Object.getPrototypeOf(wp)===EventTarget.prototype,Object.isExtensible(wp),Reflect.preventExtensions(wp),Object.isExtensible(wp)]);
T('assign',()=>{window.f=7;const r=[window.f,Object.hasOwn(window,'f')];delete window.f;return [r,d(window.f)]});
T('enumerate',()=>[Object.keys(window).includes('f'),(()=>{for(const k in window)if(k==='f')return true;return false})(),Object.getOwnPropertyNames(window).includes('f')]);
T('chainShadow',()=>{const e=document.createElement('i');e.id='zzproto';document.body.append(e);const before=d(window.zzproto);Object.prototype.zzproto=5;const viaObject=d(window.zzproto);delete Object.prototype.zzproto;EventTarget.prototype.zzproto=6;const viaTarget=d(window.zzproto);delete EventTarget.prototype.zzproto;Window.prototype.zzproto=7;const viaWindow=window.zzproto;delete Window.prototype.zzproto;return [before,viaObject,viaTarget,viaWindow,d(window.zzproto)]});
T('frames',()=>{const fr=document.querySelector('iframe[name=ifn]');fr.contentWindow.name='renamed';const a=[d(window.ifn),d(window.renamed)];fr.setAttribute('name','attrname');return [a,d(window.renamed),d(window.attrname)]});
T('frameNoName',()=>{const fr=document.createElement('iframe');fr.id='frid';document.body.append(fr);fr.contentWindow.name='cwname';return [d(window.frid),d(window.cwname)]});
T('framePrecedence',()=>{const b=document.createElement('div');b.id='twin';document.body.prepend(b);const i=document.createElement('img');i.setAttribute('name','twin');document.body.append(i);const a=document.createElement('iframe');document.body.append(a);const before=d(window.twin);a.contentWindow.name='twin';return [before,d(window.twin)]});
T('frameRealm',()=>{const w=document.getElementById('ifid').contentWindow;w.document.body.innerHTML='<p id=inner></p><img name=innerimg>';const wwp=Object.getPrototypeOf(w.Window.prototype);return [d(w.inner),d(w.innerimg),d(window.inner),d(w.f),Object.prototype.toString.call(wwp),wwp!==wp,d(w.eval('inner'))]});
T('formAssoc',()=>{const f=document.createElement('form');f.id='fa';const i=document.createElement('input');i.setAttribute('name','fa');document.body.append(f,i);return d(window.fa)});
T('nested',()=>{const o=document.createElement('object');o.setAttribute('name','outer');const e=document.createElement('embed');e.setAttribute('name','innerembed');o.append(e);document.body.append(o);return [d(window.outer),d(window.innerembed)]});
T('namespaces',()=>{const e=document.createElementNS('http://www.w3.org/1999/xhtml','img');e.setAttribute('name','nsimg');document.body.append(e);const e2=document.createElementNS('urn:x','img');e2.setAttribute('name','nsimg2');e2.setAttribute('id','foreign');document.body.append(e2);return [d(window.nsimg),d(window.nsimg2),d(window.foreign)]});
T('order',()=>{const a=document.createElement('img');a.setAttribute('name','ord');const b=document.createElement('div');b.id='ord';document.body.append(b,a);return d(window.ord)});
T('typeofMissing',()=>[typeof neverDefinedAnywhere,'neverDefinedAnywhere' in window]);
T('getterThrowsNot',()=>{try{return d(eval('missingName'))}catch(e){return e.name}});
globalThis.out=out;"#;

const CHROME_156: &str = r#"{"id":["div#f","div#f",true,"div#f","div#f","p#F2","undefined"],"dup":[["HTMLCollection",2,"div","span"],true,"object"],"names":[["imgn","img#imgn"],["formn","form#formn"],["embn","embed#embn"],["objn","object#objn"],["divn","undefined"],["inputn","undefined"],["an","undefined"],["ifid","iframe#ifid"],["svgimg","undefined"]],"frameName":"TypeError:w.setAttribute is not a function","both":["img#both2","img#both2","img#imgid","img#imgid",["HTMLCollection",2,"img","div"],["HTMLCollection",2,"img","img"]],"builtins":[["addEventListener","function"],["toString","function"],["document","object"],["location","object"],["alert","function"],["hasOwnProperty","function"],["name","string"],["Window","function"],["onload","null"],["frames","window"],["constructor","function"],["valueOf","function"]],"proto":"[object Window]","index":["windowproxy","undefined","circle#svgc"],"decl":["undefined","p#letdecl","number","function"],"scopes":["undefined","undefined"],"live":["undefined","b#later",["undefined","b#later2"],"b#later3","undefined"],"liveName":["undefined","img#liven","undefined"],"detached":["undefined","undefined"],"collection":[2,3,2,"i#cs",true,true,true,"[object HTMLCollection]"],"bare":["u#bare1","object"],"in":[true,false,false,true,true,true],"wpShape":["[object WindowProperties]",[],["Symbol(Symbol.toStringTag)"],true,"undefined",true],"wpDescriptor":[true,false,true,true,true,true,true,false],"wpGetters":["div#f","div#f","undefined"],"wpSloppySet":[false,"undefined","div#f"],"wpStrictSet":"TypeError:Failed to set a named property 'zz' on 'WindowProperties': Named property setter is not supported.","wpDefine":[true,false,true,"div#f",1],"wpDelete":[false,"div#f",false],"wpProto":[false,true,true,true,false,true],"assign":[[7,true],"div#f"],"enumerate":[false,false,false],"chainShadow":["i#zzproto","number","number",7,"i#zzproto"],"frames":[["undefined","windowproxy"],"windowproxy","undefined"],"frameNoName":["iframe#frid","windowproxy"],"framePrecedence":[["HTMLCollection",2,"div","img"],"windowproxy"],"frameRealm":["p#inner","img#innerimg","undefined","undefined","[object WindowProperties]",true,"p#inner"],"formAssoc":"form#fa","nested":["object#outer","embed#innerembed"],"namespaces":["img#nsimg","undefined","img#foreign"],"order":["HTMLCollection",2,"div","img"],"typeofMissing":["undefined",false],"getterThrowsNot":"ReferenceError"}"#;

#[tokio::test(flavor = "current_thread")]
async fn window_named_access_matches_chrome156() {
    let html = format!("<!doctype html><body>{BODY}<script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://named.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    let chrome: Value = serde_json::from_str(CHROME_156).unwrap();
    for (key, want) in chrome.as_object().unwrap() {
        assert_eq!(got.get(key), Some(want), "{key}");
    }
    assert_eq!(got, chrome);
}

/// Page ids that collide with names the engine itself relies on: the host result
/// channel `__RESULT` (never resolved), names of former bookkeeping globals (now
/// private, so they are ordinary page names), interfaces the bootstrap installs
/// only when absent (still installed: named access starts after the bootstrap),
/// and real globals (own properties shadow named items).
const ENGINE_NAMES: &str = r#"<div id=ok1></div><div id=__pendingFetches></div><div id=__RESULT></div>
<div id=__depth></div><div id=__hi></div><div id=__probe></div><div id=Notification></div>
<div id=DOMQuad></div><div id=scheduler></div><div id=setTimeout></div><img name=fetch>
<form name=queueMicrotask></form><div id=document></div><div id=Promise></div><div id=indexedDB></div>"#;

#[tokio::test(flavor = "current_thread")]
async fn named_access_never_redirects_engine_globals() {
    let html = format!(
        r#"<!doctype html><body>{ENGINE_NAMES}<script>
globalThis.out={{
  control: window.ok1 === document.getElementById('ok1'),
  bookkeeping: ['__pendingFetches','__RESULT','__depth','__hi','__probe'].map(n=>typeof window[n]),
  installed: [typeof Notification, typeof DOMQuad, typeof scheduler, typeof indexedDB, typeof setTimeout, typeof fetch, typeof queueMicrotask, typeof Promise.resolve],
  document: document.nodeType,
}};
fetch('data:text/plain,hi').then(r=>r.text()).then(()=>{{out.fetched='settled'}},()=>{{out.fetched='settled'}});
queueMicrotask(()=>{{out.microtask=true}});
setTimeout(()=>{{out.timer=true}},0);
</script></body>"#
    );
    let mut page = PageSession::open(&html, "https://engine-names.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_engine("__tsNs.runTimers(200)")
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    assert_eq!(
        got,
        serde_json::json!({
            "control": true,
            "bookkeeping": ["object", "undefined", "object", "object", "object"],
            "installed": ["function", "function", "object", "object", "function", "function", "function", "function"],
            "document": 9,
            "fetched": "settled",
            "microtask": true,
            "timer": true
        })
    );
    // Fetch accounting is private: a page element named `__pendingFetches` is not it.
    assert_eq!(
        page.eval_engine_json_expression("__tsNs.pendingFetchCount()")
            .await
            .unwrap(),
        r#""0""#
    );
    // The host result channel reads `__RESULT` back: a page element is not a result.
    let result = turbo_surf_render::eval_async(
        &format!("<body>{ENGINE_NAMES}</body>"),
        "https://engine-names.test/",
        "void 0",
    )
    .await
    .unwrap();
    assert_eq!(result, "");
}

/// The resolver runs on every global miss; it must keep answering (and never
/// throw) after the page replaces the DOM methods, collections and builtins it
/// would otherwise look up.
/// Chrome 156 (156.0.8078.47, headless, localhost page) yields the same values.
#[tokio::test(flavor = "current_thread")]
async fn named_access_ignores_page_patched_intrinsics() {
    let html = r#"<!doctype html><body><div id=one></div><i id=two></i><b id=two></b><script>
const late=document.createElement('p');document.body.appendChild(late);
globalThis.run=()=>{
const saved=[Document.prototype.getElementsByTagName,Element.prototype.getAttribute,Object.getOwnPropertyDescriptor(Element.prototype,'localName'),Object.getOwnPropertyDescriptor(Element.prototype,'namespaceURI'),Map.prototype.get,Map.prototype.set,Reflect.apply,Object.defineProperty,Object.getOwnPropertyDescriptor,Object.hasOwn];
const define=Object.defineProperty;
Document.prototype.getElementsByTagName=()=>{throw 1};
Element.prototype.getAttribute=()=>'one';
define(Element.prototype,'localName',{get(){throw 2},configurable:true});
define(Element.prototype,'namespaceURI',{get(){throw 3},configurable:true});
Map.prototype.get=()=>{throw 4};Map.prototype.set=()=>{throw 5};
Reflect.apply=()=>{throw 6};Object.defineProperty=()=>{throw 7};Object.getOwnPropertyDescriptor=()=>{throw 8};
Object.hasOwn=()=>{throw 9};
let result;
try{late.setAttribute('id','late');let missing;try{missing=typeof notDefinedAnywhere}catch(e){missing='threw '+e}
result=[typeof one,two.length,late===window.late,missing];}
finally{Document.prototype.getElementsByTagName=saved[0];Element.prototype.getAttribute=saved[1];define(Element.prototype,'localName',saved[2]);define(Element.prototype,'namespaceURI',saved[3]);Map.prototype.get=saved[4];Map.prototype.set=saved[5];Reflect.apply=saved[6];Object.defineProperty=saved[7];Object.getOwnPropertyDescriptor=saved[8];Object.hasOwn=saved[9];}
return result;};
</script></body>"#;
    let mut page = PageSession::open(html, "https://patched.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("run()").await.unwrap()).unwrap();
    assert_eq!(got, serde_json::json!(["object", 2, true, "undefined"]));
}
