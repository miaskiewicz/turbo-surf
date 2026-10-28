//! Structured clone of host values: a WebAssembly.Module crosses postMessage and
//! structuredClone by compiled-module id, a DOMRect crosses a MessagePort (other
//! geometry interfaces: turbo-surf#66), and functions, symbols and nodes are not
//! cloneable. Expected: Chrome 156 (156.0.8078.4, headless) running
//! the same script on a localhost page.
use serde_json::Value;

const SCRIPT: &str = r#"(async()=>{const log=[];const t=(n,f)=>{try{log.push([n,f()])}catch(e){log.push([n,e.name])}};
const bytes=new Uint8Array([0,97,115,109,1,0,0,0,1,5,1,96,0,1,127,3,2,1,0,7,7,1,3,97,110,115,0,0,10,6,1,4,0,65,42,11]);
const module=new WebAssembly.Module(bytes);
const got=new Promise(r=>{onmessage=e=>r(e.data)});
postMessage({module},'*');
const data=await got;
log.push(['module',data.module instanceof WebAssembly.Module, data.module===module, new WebAssembly.Instance(data.module).exports.ans()]);
const {port1,port2}=new MessageChannel();
const rect=new Promise(r=>{port2.onmessage=e=>r(e.data)});
port1.postMessage([new DOMRect(1,2,3,4)]);
const [r]=await rect;
log.push(['rect',r.constructor.name,r.x,r.y,r.width,r.height]);
t('function',()=>{structuredClone(()=>1);return 'ok'});
t('symbol',()=>{structuredClone(Symbol());return 'ok'});
t('node',()=>{structuredClone(document.body);return 'ok'});
t('ws module structuredClone',()=>structuredClone(module) instanceof WebAssembly.Module);
globalThis.__RESULT=JSON.stringify(log);})()"#;

const CHROME_156: &str = r#"[["module",true,false,42],["rect","DOMRect",1,2,3,4],["function","DataCloneError"],["symbol","DataCloneError"],["node","DataCloneError"],["ws module structuredClone",true]]"#;

#[tokio::test(flavor = "current_thread")]
async fn wasm_modules_geometry_and_uncloneable_values_match_chrome() {
    let result = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "https://clone.test/",
        SCRIPT,
    )
    .await
    .unwrap();
    let log: Value = serde_json::from_str(&result).unwrap();
    let chrome: Value = serde_json::from_str(CHROME_156).unwrap();
    assert_eq!(log, chrome);
}

/// Chrome 156 capture: a port cannot carry itself, and a getter that transfers a
/// listed buffer or port elsewhere mid-serialization fails the clone.
const OWNERSHIP: &str = r#"(()=>{const log=[];const t=(n,f)=>{try{log.push([n,f()])}catch(e){log.push([n,e.name])}};
const c=new MessageChannel();
t('self port',()=>{c.port1.postMessage(0,[c.port1]);return 'ok'});
const b=new ArrayBuffer(8);
t('detached by getter',()=>{postMessage({get x(){structuredClone(0,{transfer:[b]});return 1}},'*',[b]);return 'ok'});
t('detached after',()=>b.byteLength);
const p=new MessageChannel();
t('port moved by getter',()=>{postMessage({get x(){structuredClone(0,{transfer:[p.port1]});return 1}},'*',[p.port1]);return 'ok'});
t('structuredClone transfer',()=>{const a=new ArrayBuffer(4);const r=structuredClone({a},{transfer:[a]});return [a.byteLength,r.a.byteLength]});
t('structuredClone non-array transfer',()=>structuredClone(1,{transfer:5}));
globalThis.__RESULT=JSON.stringify(log);})()"#;

const OWNERSHIP_CHROME_156: &str = r#"[["self port","DataCloneError"],["detached by getter","DataCloneError"],["detached after",0],["port moved by getter","DataCloneError"],["structuredClone transfer",[0,4]],["structuredClone non-array transfer","TypeError"]]"#;

#[tokio::test(flavor = "current_thread")]
async fn transfer_ownership_changed_during_serialization_matches_chrome() {
    let result = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "https://clone.test/",
        OWNERSHIP,
    )
    .await
    .unwrap();
    assert_eq!(result, OWNERSHIP_CHROME_156);
}

/// Native bounds (not Chrome limits): at most 4096 transfer-list entries and 64 MiB
/// of serialized message per postMessage, each rejected before anything moves (window
/// postMessage reports the transfer-list bound as a TypeError).
#[tokio::test(flavor = "current_thread")]
async fn native_transfer_and_serialization_budgets_reject_before_transfer() {
    let result = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "https://clone.test/",
        r#"(()=>{const out=[];
const buffers=Array.from({length:4097},()=>new ArrayBuffer(1));
try{postMessage(0,'*',buffers);out.push('sent')}catch(e){out.push(e.name+': '+e.message,buffers[0].byteLength)}
const big=new Uint8Array(64*1024*1024+1);
try{structuredClone(big);out.push('cloned')}catch(e){out.push(e.name+': '+e.message)}
globalThis.__RESULT=JSON.stringify(out);})()"#,
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        r#"["TypeError: Native transfer sequence budget exceeded",1,"RangeError: Native message serialization budget exceeded"]"#
    );
}
