//! MessagePort queues: a listener without start() waits, closing a port drops its
//! queued and later messages (also from inside a handler), clearing onmessage keeps the
//! queue, and a transferred port keeps its entanglement. Expected: Chrome 156
//! (156.0.8078.4, headless) running the same script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"(async()=>{const log=[];const tick=()=>new Promise(r=>setTimeout(r,20));
const a=new MessageChannel();a.port2.addEventListener('message',e=>log.push('listener-nostart:'+e.data));a.port1.postMessage('x');await tick();
a.port2.start();await tick();
const b=new MessageChannel();b.port2.onmessage=e=>log.push('b:'+e.data);b.port1.postMessage(1);b.port1.postMessage(2);b.port2.close();await tick();
const c=new MessageChannel();c.port1.postMessage('q1');c.port2.onmessage=null;c.port2.onmessage=e=>log.push('c:'+e.data);await tick();
const d=new MessageChannel();d.port2.onmessage=e=>{log.push('d:'+e.data);if(e.data===1)d.port2.close()};d.port1.postMessage(1);d.port1.postMessage(2);await tick();
const e=new MessageChannel();const inner=new MessageChannel();e.port2.onmessage=ev=>{const p=ev.ports[0];p.onmessage=m=>log.push('inner:'+m.data)};e.port1.postMessage('carry',[inner.port1]);await tick();inner.port2.postMessage('through');await tick();
const f=new MessageChannel();f.port1.close();f.port2.onmessage=ev=>log.push('f:'+ev.data);f.port2.postMessage('to-closed');f.port1.postMessage('from-closed');await tick();
return log;})()"#;

const CHROME_156: &str = r#"["listener-nostart:x","c:q1","d:1","inner:through"]"#;

#[tokio::test(flavor = "current_thread")]
async fn port_queues_start_close_and_transfer_like_chrome() {
    let mut page = PageSession::open("<body>", "https://ports.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression(SCRIPT).await.unwrap(), CHROME_156);
}
