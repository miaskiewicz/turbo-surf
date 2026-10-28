//! postMessage / MessagePort.postMessage / structuredClone transfer-list validation
//! (turbo-surf#62): Chrome's "Failed to execute ..." prefix, the transfer index, the
//! check order (list scan for duplicates and types, then buffers, locked streams,
//! the source port and neutered ports) and the TypeError for locked streams,
//! non-detachable buffers and primitives. Expected log: Chrome 156 (156.0.8078.47,
//! headless) running the same script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const log=[];const t=(n,f)=>{try{f();log.push([n,'ok'])}catch(e){log.push([n,e.name+': '+e.message])}};
const c=new MessageChannel();postMessage(0,'*',[c.port1]);
const e=new ArrayBuffer(1);postMessage(0,'*',[e]);
const L=()=>{const s=new ReadableStream();s.getReader();return s};
const W=()=>new WebAssembly.Memory({initial:1}).buffer;
const P=()=>new MessageChannel().port1;
t('neutered',()=>postMessage(0,'*',[c.port1]));
t('neutered options',()=>postMessage(0,{transfer:[c.port1]}));
t('neutered after buffers',()=>postMessage(0,'*',[new ArrayBuffer(1),new ArrayBuffer(1),c.port1]));
t('neutered port index',()=>postMessage(0,'*',[P(),c.port1]));
t('duplicate port',()=>{const p=P();postMessage(0,'*',[p,new ArrayBuffer(1),p])});
t('duplicate buffer',()=>{const b=new ArrayBuffer(1);postMessage(0,'*',[P(),P(),b,b])});
t('duplicate readable',()=>{const s=new ReadableStream();postMessage(0,'*',[new ArrayBuffer(1),s,s])});
t('duplicate writable',()=>{const s=new WritableStream();postMessage(0,'*',[s,s])});
t('duplicate transform',()=>{const s=new TransformStream();postMessage(0,'*',[s,s])});
t('detached',()=>postMessage(0,'*',[P(),e]));
t('not detachable',()=>postMessage(0,'*',[new ArrayBuffer(1),W()]));
t('not transferable',()=>postMessage(0,'*',[new ReadableStream(),{}]));
t('primitive',()=>postMessage(0,'*',[1]));
t('primitive options',()=>postMessage(0,{transfer:[new ArrayBuffer(1),1]}));
t('port primitive options',()=>P().postMessage(0,{transfer:[1]}));
t('clone primitive',()=>structuredClone(0,{transfer:[{},1]}));
t('locked readable',()=>postMessage(0,'*',[L()]));
t('locked writable',()=>{const s=new WritableStream();s.getWriter();postMessage(0,'*',[s])});
t('locked transform',()=>{const s=new TransformStream();s.readable.getReader();postMessage(0,'*',[s])});
t('type before duplicate',()=>{const b=new ArrayBuffer(1);postMessage(0,'*',[{},b,b])});
t('duplicate before type',()=>{const b=new ArrayBuffer(1);postMessage(0,'*',[b,b,{}])});
t('type before neutered',()=>postMessage(0,'*',[c.port1,{}]));
t('duplicate before detached',()=>{const p=P();postMessage(0,'*',[e,p,p])});
t('detached before locked',()=>postMessage(0,'*',[L(),e]));
t('detached before neutered',()=>postMessage(0,'*',[c.port1,e]));
t('locked before neutered',()=>postMessage(0,'*',[c.port1,L()]));
t('detached before not detachable',()=>postMessage(0,'*',[W(),e]));
t('not detachable before neutered',()=>postMessage(0,'*',[c.port1,W()]));
t('port neutered',()=>P().postMessage(0,[c.port1]));
t('port duplicate',()=>{const p=P();P().postMessage(0,[p,p])});
t('port duplicate buffer',()=>{const b=new ArrayBuffer(1);P().postMessage(0,[b,b])});
t('port detached',()=>P().postMessage(0,[e]));
t('port not transferable',()=>P().postMessage(0,[{}]));
t('port primitive',()=>P().postMessage(0,[null]));
t('port locked',()=>P().postMessage(0,[L()]));
t('port not detachable',()=>P().postMessage(0,[W()]));
t('source port',()=>{const x=new MessageChannel();x.port1.postMessage(0,[new ArrayBuffer(1),x.port1])});
t('source before neutered',()=>{const x=new MessageChannel();x.port1.postMessage(0,[c.port1,x.port1])});
t('detached before source',()=>{const x=new MessageChannel();x.port1.postMessage(0,[e,x.port1])});
t('locked before source',()=>{const x=new MessageChannel();x.port1.postMessage(0,[x.port1,L()])});
t('neutered source',()=>c.port1.postMessage(0,[c.port1]));
t('clone neutered',()=>structuredClone(0,{transfer:[c.port1]}));
t('clone duplicate',()=>{const p=P();structuredClone(0,{transfer:[p,p]})});
t('clone duplicate buffer',()=>{const b=new ArrayBuffer(1);structuredClone(0,{transfer:[b,b]})});
t('clone detached',()=>structuredClone(0,{transfer:[e]}));
t('clone not transferable',()=>structuredClone(0,{transfer:[{}]}));
t('clone locked',()=>structuredClone(0,{transfer:[L()]}));
t('clone not detachable',()=>structuredClone(0,{transfer:[W()]}));
globalThis.log=log;"#;

const CHROME_156: &str = r#"[["neutered","DataCloneError: Failed to execute 'postMessage' on 'Window': Port at index 0 is already neutered."],["neutered options","DataCloneError: Failed to execute 'postMessage' on 'Window': Port at index 0 is already neutered."],["neutered after buffers","DataCloneError: Failed to execute 'postMessage' on 'Window': Port at index 0 is already neutered."],["neutered port index","DataCloneError: Failed to execute 'postMessage' on 'Window': Port at index 1 is already neutered."],["duplicate port","DataCloneError: Failed to execute 'postMessage' on 'Window': Message port at index 2 is a duplicate of an earlier port."],["duplicate buffer","DataCloneError: Failed to execute 'postMessage' on 'Window': ArrayBuffer at index 3 is a duplicate of an earlier ArrayBuffer."],["duplicate readable","DataCloneError: Failed to execute 'postMessage' on 'Window': ReadableStream at index 2 is a duplicate of an earlier ReadableStream."],["duplicate writable","DataCloneError: Failed to execute 'postMessage' on 'Window': WritableStream at index 1 is a duplicate of an earlier WritableStream."],["duplicate transform","DataCloneError: Failed to execute 'postMessage' on 'Window': TransformStream at index 1 is a duplicate of an earlier TransformStream."],["detached","DataCloneError: Failed to execute 'postMessage' on 'Window': ArrayBuffer at index 0 is already detached."],["not detachable","TypeError: Failed to execute 'postMessage' on 'Window': ArrayBuffer at index 1 is not detachable and could not be transferred."],["not transferable","DataCloneError: Failed to execute 'postMessage' on 'Window': Value at index 1 does not have a transferable type."],["primitive","TypeError: Failed to execute 'postMessage' on 'Window': Failed to convert value to 'object'."],["primitive options","TypeError: Failed to execute 'postMessage' on 'Window': Failed to read the 'transfer' property from 'StructuredSerializeOptions': Failed to convert value to 'object'."],["port primitive options","TypeError: Failed to execute 'postMessage' on 'MessagePort': Failed to read the 'transfer' property from 'StructuredSerializeOptions': Failed to convert value to 'object'."],["clone primitive","TypeError: Failed to execute 'structuredClone' on 'Window': Failed to read the 'transfer' property from 'StructuredSerializeOptions': Failed to convert value to 'object'."],["locked readable","TypeError: Failed to execute 'postMessage' on 'Window': Cannot transfer a locked stream"],["locked writable","TypeError: Failed to execute 'postMessage' on 'Window': Cannot transfer a locked stream"],["locked transform","TypeError: Failed to execute 'postMessage' on 'Window': Cannot transfer a locked stream"],["type before duplicate","DataCloneError: Failed to execute 'postMessage' on 'Window': Value at index 0 does not have a transferable type."],["duplicate before type","DataCloneError: Failed to execute 'postMessage' on 'Window': ArrayBuffer at index 1 is a duplicate of an earlier ArrayBuffer."],["type before neutered","DataCloneError: Failed to execute 'postMessage' on 'Window': Value at index 1 does not have a transferable type."],["duplicate before detached","DataCloneError: Failed to execute 'postMessage' on 'Window': Message port at index 2 is a duplicate of an earlier port."],["detached before locked","DataCloneError: Failed to execute 'postMessage' on 'Window': ArrayBuffer at index 0 is already detached."],["detached before neutered","DataCloneError: Failed to execute 'postMessage' on 'Window': ArrayBuffer at index 0 is already detached."],["locked before neutered","TypeError: Failed to execute 'postMessage' on 'Window': Cannot transfer a locked stream"],["detached before not detachable","DataCloneError: Failed to execute 'postMessage' on 'Window': ArrayBuffer at index 1 is already detached."],["not detachable before neutered","TypeError: Failed to execute 'postMessage' on 'Window': ArrayBuffer at index 0 is not detachable and could not be transferred."],["port neutered","DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Port at index 0 is already neutered."],["port duplicate","DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Message port at index 1 is a duplicate of an earlier port."],["port duplicate buffer","DataCloneError: Failed to execute 'postMessage' on 'MessagePort': ArrayBuffer at index 1 is a duplicate of an earlier ArrayBuffer."],["port detached","DataCloneError: Failed to execute 'postMessage' on 'MessagePort': ArrayBuffer at index 0 is already detached."],["port not transferable","DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Value at index 0 does not have a transferable type."],["port primitive","TypeError: Failed to execute 'postMessage' on 'MessagePort': Failed to convert value to 'object'."],["port locked","TypeError: Failed to execute 'postMessage' on 'MessagePort': Cannot transfer a locked stream"],["port not detachable","TypeError: Failed to execute 'postMessage' on 'MessagePort': ArrayBuffer at index 0 is not detachable and could not be transferred."],["source port","DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Port at index 0 contains the source port."],["source before neutered","DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Port at index 1 contains the source port."],["detached before source","DataCloneError: Failed to execute 'postMessage' on 'MessagePort': ArrayBuffer at index 0 is already detached."],["locked before source","TypeError: Failed to execute 'postMessage' on 'MessagePort': Cannot transfer a locked stream"],["neutered source","ok"],["clone neutered","DataCloneError: Failed to execute 'structuredClone' on 'Window': Port at index 0 is already neutered."],["clone duplicate","DataCloneError: Failed to execute 'structuredClone' on 'Window': Message port at index 1 is a duplicate of an earlier port."],["clone duplicate buffer","DataCloneError: Failed to execute 'structuredClone' on 'Window': ArrayBuffer at index 1 is a duplicate of an earlier ArrayBuffer."],["clone detached","DataCloneError: Failed to execute 'structuredClone' on 'Window': ArrayBuffer at index 0 is already detached."],["clone not transferable","DataCloneError: Failed to execute 'structuredClone' on 'Window': Value at index 0 does not have a transferable type."],["clone locked","TypeError: Failed to execute 'structuredClone' on 'Window': Cannot transfer a locked stream"],["clone not detachable","TypeError: Failed to execute 'structuredClone' on 'Window': ArrayBuffer at index 0 is not detachable and could not be transferred."]]"#;

#[tokio::test(flavor = "current_thread")]
async fn transfer_list_errors_match_chrome() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://transfer.test/", "", "", 5000)
        .await
        .unwrap();
    let log: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression("log").await.unwrap()).unwrap();
    let chrome: serde_json::Value = serde_json::from_str(CHROME_156).unwrap();
    let rows = |value: &serde_json::Value| value.as_array().unwrap().clone();
    for (got, want) in rows(&log).iter().zip(rows(&chrome).iter()) {
        assert_eq!(got, want);
    }
    assert_eq!(log, chrome);
}
