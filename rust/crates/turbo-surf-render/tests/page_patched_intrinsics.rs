//! Engine-internal JavaScript never looks up page-replaceable intrinsics at run time
//! (turbo-surf#87). Before anything else runs, the page replaces every configurable
//! method and accessor of the Array/Map/Set/WeakMap/WeakSet prototypes, the
//! Object/Reflect/Array/Promise/JSON/Symbol/Number/Math statics, the String, Function,
//! Promise and RegExp prototypes and the EventTarget/Node/Element/Document/HTMLElement
//! prototypes with throwing functions, and installs throwing index accessors
//! ('0'..'3') on Array.prototype and Object.prototype. Loading must still reach
//! `interactive` and `complete` with the lifecycle events, and appending a div, a text
//! node, an inline script and an iframe (from the head script, DOMContentLoaded and
//! load) and insertBefore/remove must work. The page's own code only calls originals
//! it captured first (its elements are created before the patching). Expected values:
//! Chrome 156 (156.0.8078.47, headless) loading the same page from localhost in an
//! iframe and restoring the builtins before reading the results.
//!
//! The members turbo-dom's bootstrap still reads on this path (miaskiewicz/turbo-dom#172)
//! are left in place by the first test; the second patches the full set.
use serde_json::Value;
use turbo_surf_render::PageSession;

const PAGE: &str = r##"<!doctype html><html><head><script>
(() => {
const A=Reflect.apply,gopd=Object.getOwnPropertyDescriptor,def=Object.defineProperty,own=Reflect.ownKeys,G=globalThis,Err=Error;
const fn=(o,k)=>gopd(o,k).value,getter=(o,k)=>gopd(o,k).get,setter=(o,k)=>gopd(o,k).set;
const create=fn(Document.prototype,'createElement'),createText=fn(Document.prototype,'createTextNode'),append=fn(Node.prototype,'appendChild'),
  insertBefore=fn(Node.prototype,'insertBefore'),remove=fn(Element.prototype,'remove'),setAttr=fn(Element.prototype,'setAttribute'),
  listen=fn(EventTarget.prototype,'addEventListener'),parent=getter(Node.prototype,'parentNode'),bodyOf=getter(Document.prototype,'body'),
  headOf=getter(Document.prototype,'head'),ready=getter(Document.prototype,'readyState'),setText=setter(Node.prototype,'textContent'),
  childCount=getter(Element.prototype,'childElementCount'),contentWindow=getter(HTMLIFrameElement.prototype,'contentWindow'),
  tag=getter(Element.prototype,'tagName');
const out={__proto__:null,events:''};G.__out=out;
const log=(s)=>{out.events+=s+';'};
const saved={__proto__:null};let count=0;
const save=(o,k)=>{saved[count]=[o,k,gopd(o,k)];count++};
const thrower=(name)=>function(){throw new Err('patched '+name)};
const targets=[[Array.prototype,'Array.prototype'],[Array,'Array'],[Map.prototype,'Map.prototype'],[Set.prototype,'Set.prototype'],
  [WeakMap.prototype,'WeakMap.prototype'],[WeakSet.prototype,'WeakSet.prototype'],[Object,'Object'],[Reflect,'Reflect'],
  [String.prototype,'String.prototype'],[Function.prototype,'Function.prototype'],[Promise,'Promise'],[Promise.prototype,'Promise.prototype'],
  [JSON,'JSON'],[Symbol,'Symbol'],[Number,'Number'],[Math,'Math'],[RegExp.prototype,'RegExp.prototype'],
  [EventTarget.prototype,'EventTarget.prototype'],[Node.prototype,'Node.prototype'],[Element.prototype,'Element.prototype'],
  [Document.prototype,'Document.prototype'],[HTMLElement.prototype,'HTMLElement.prototype']];
const skip={__proto__:null,constructor:1,prototype:1,length:1,name:1};
const kept={__proto__:null,/*KEPT*/};
const patchAll=()=>{
  for(let t=0;t<targets.length;t++){const o=targets[t][0],label=targets[t][1],ks=own(o);
    for(let i=0;i<ks.length;i++){const k=ks[i];if(typeof k==='string'&&skip[k])continue;if(k===Symbol.hasInstance)continue;
      const d=gopd(o,k);if(!d.configurable)continue;const n=label+'.'+(typeof k==='symbol'?'@@':'')+String(k);if(kept[n]||kept[label+'.*'])continue;
      if(typeof d.value==='function'){save(o,k);def(o,k,{value:thrower(n),writable:true,configurable:true,enumerable:d.enumerable})}
      else if(d.get||d.set){save(o,k);def(o,k,{get:thrower('get '+n),set:thrower('set '+n),configurable:true,enumerable:d.enumerable})}}}
  const indexed=[Array.prototype,Object.prototype];
  for(let t=0;t<2;t++)for(let i=0;i<4;i++){const o=indexed[t],k=''+i;save(o,k);def(o,k,{get:thrower('get index'),set:thrower('set index'),configurable:true})}
};
G.__restore=()=>{for(let i=count-1;i>=0;i--){const r=saved[i];if(r[2])def(r[0],r[1],r[2]);else delete r[0][r[1]]}};
const attempt=(name,f)=>{try{out[name]=f()}catch(e){out[name]='threw '+(e&&e.message)}};
// Elements are created before the builtins are replaced: element creation is not the
// path under test (and turbo-dom's factory has its own issue).
const made={__proto__:null};const phases=['head','dcl','load'];
for(let p=0;p<3;p++){const phase=phases[p];
  made[phase+'div']=A(create,document,['div']);made[phase+'span']=A(create,document,['span']);made[phase+'text']=A(createText,document,['hi']);
  made[phase+'script']=A(create,document,['script']);A(setText,made[phase+'script'],["__out.ran"+phase+"=1"]);
  made[phase+'iframe']=A(create,document,['iframe']);made[phase+'p']=A(create,document,['p']);}
const checks=(phase)=>{
  const container=phase==='head'?A(headOf,document,[]):A(bodyOf,document,[]);
  attempt(phase+'Div',()=>{const d=made[phase+'div'];A(append,container,[d]);return A(parent,d,[])===container});
  attempt(phase+'Text',()=>{const t=made[phase+'text'],d=made[phase+'span'];A(append,d,[t]);A(append,container,[d]);return A(parent,t,[])===d});
  attempt(phase+'Script',()=>{A(append,container,[made[phase+'script']]);return out['ran'+phase]===1});
  attempt(phase+'Iframe',()=>{const f=made[phase+'iframe'];A(append,container,[f]);const w=A(contentWindow,f,[]);return w!==null&&typeof w==='object'});
  attempt(phase+'Insert',()=>{const d=made[phase+'p'];A(insertBefore,container,[d,null]);A(remove,d,[]);return A(parent,d,[])===null});
};
A(listen,document,['readystatechange',()=>log('rs:'+A(ready,document,[]))]);
A(listen,document,['DOMContentLoaded',()=>{log('dcl');checks('dcl')}]);
A(listen,window,['load',()=>{log('load:'+A(ready,document,[]));checks('load')}]);
patchAll();
out.patched=count;
checks('head');
})();
</script></head><body><p>static</p><script>__out.bodyScript=1</script></body></html>"##;

/// Members turbo-dom's own bootstrap JS still calls on this path (turbo-dom#172).
const TURBO_DOM_MEMBERS: &str = "'Array.prototype.includes':1,'Function.prototype.apply':1,'Function.prototype.call':1,'Object.create':1,'Object.defineProperty':1,'Object.getOwnPropertyDescriptor':1,'RegExp.prototype.*':1,'String.prototype.replace':1,'WeakMap.prototype.get':1,'WeakMap.prototype.set':1,'Element.prototype.children':1,'Node.prototype.parentNode':1,'HTMLElement.prototype.onload':1";

const CHROME_156: &str = r##"{"events":"rs:interactive;dcl;rs:complete;load:complete;","headDiv":true,"headText":true,"ranhead":1,"headScript":true,"headIframe":true,"headInsert":true,"bodyScript":1,"dclDiv":true,"dclText":true,"randcl":1,"dclScript":true,"dclIframe":true,"dclInsert":true,"loadDiv":true,"loadText":true,"ranload":1,"loadScript":true,"loadIframe":true,"loadInsert":true}"##;

async fn load_patched(kept: &str) -> Value {
    let page_html = PAGE.replace("/*KEPT*/", kept);
    let mut page = PageSession::open(&page_html, "https://patched.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval("__restore()").await.unwrap();
    let mut got: Value =
        serde_json::from_str(&page.eval_json_expression("__out").await.unwrap()).unwrap();
    // How many members the page replaced depends on the realm's interface shapes; it
    // must have replaced the hostile set (Chrome: 810 and 841).
    let patched = got
        .as_object_mut()
        .and_then(|out| out.remove("patched"))
        .and_then(|count| count.as_u64())
        .unwrap_or(0);
    assert!(patched > 600, "patched {patched} members");
    got
}

fn assert_chrome(got: &Value) {
    let chrome: Value = serde_json::from_str(CHROME_156).unwrap();
    for (key, want) in chrome.as_object().unwrap() {
        assert_eq!(got.get(key), Some(want), "{key}");
    }
    assert_eq!(got, &chrome);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#87: fails until the fix"]
async fn page_patched_intrinsics_do_not_break_loading_or_append_child() {
    assert_chrome(&load_patched(TURBO_DOM_MEMBERS).await);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "miaskiewicz/turbo-dom#172: turbo-dom's bootstrap still calls page-patchable intrinsics"]
async fn page_patched_intrinsics_full_set_like_chrome156() {
    assert_chrome(&load_patched("").await);
}
