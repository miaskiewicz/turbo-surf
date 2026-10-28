//! MessagePort lifecycle edges: a transferred (neutered) port, a closed entangled
//! port, a queue started late, and foreign receivers. The expected log is Chrome 156
//! (156.0.8078.4, headless) running the same script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const log=[];const t=(name,f)=>{try{log.push([name,f()])}catch(e){log.push([name,'throw '+e.name+': '+e.message])}};
const {port1,port2}=new MessageChannel();
port2.onmessage=e=>log.push(['port2 got',e.data,e.ports.length]);
window.onmessage=e=>{const p=e.ports[0];log.push(['window got',e.data,e.ports.length]);p.onmessage=m=>log.push(['moved got',m.data]);p.postMessage('back');};
postMessage('carry','*',[port1]);
t('start transferred',()=>{port1.start();return 'ok'});
t('close transferred',()=>{port1.close();return 'ok'});
t('post transferred',()=>{port1.postMessage('lost');return 'ok'});
t('retransfer',()=>{postMessage('again','*',[port1]);return 'ok'});
const c2=new MessageChannel();c2.port1.close();
t('post to closed',()=>{c2.port2.postMessage('x');return 'ok'});
t('start closed',()=>{c2.port1.start();return 'ok'});
t('close twice',()=>{c2.port1.close();c2.port1.close();return 'ok'});
const c3=new MessageChannel();c3.port2.addEventListener('message',e=>log.push(['c3 listener',e.data]));
c3.port1.postMessage('queued-before-start');
setTimeout(()=>{c3.port2.start();},10);
t('illegal start',()=>MessagePort.prototype.start.call({}));
t('illegal post',()=>MessagePort.prototype.postMessage.call({}, 1));
t('illegal close',()=>MessagePort.prototype.close.call(document));
globalThis.log=log;"#;

const CHROME_156: &str = r#"[["start transferred","ok"],["close transferred","ok"],["post transferred","ok"],["retransfer","throw DataCloneError: Failed to execute 'postMessage' on 'Window': Port at index 0 is already neutered."],["post to closed","ok"],["start closed","ok"],["close twice","ok"],["illegal start","throw TypeError: Illegal invocation"],["illegal post","throw TypeError: Illegal invocation"],["illegal close","throw TypeError: Illegal invocation"],["window got","carry",1],["port2 got","back",0],["c3 listener","queued-before-start"]]"#;

#[tokio::test(flavor = "current_thread")]
async fn neutered_closed_and_late_started_ports_match_chrome() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://ports.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval("globalThis[Symbol.for('ts')].runTimers(200)")
        .await
        .unwrap();
    let log: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression("log").await.unwrap()).unwrap();
    let chrome: serde_json::Value = serde_json::from_str(CHROME_156).unwrap();
    assert_eq!(log, chrome);
}
