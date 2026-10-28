//! Window message and timer tasks across same-agent windows run in posting order
//! (turbo-surf#72): a message to a child, a message the child posts to its parent,
//! same-window messages and both realms' zero-delay timers interleave as one FIFO,
//! and posting a message does not consume a timer id. Expected log: Chrome 156
//! (156.0.8078.47, headless) running the same script on a localhost page. (Chrome's
//! `event.source` for the child's post is the incumbent child window; the stock SDK
//! has no incumbent-realm API, so the log records data only.)
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const log=[];globalThis.log=log;const f=document.createElement('iframe');document.body.append(f);const w=f.contentWindow;
onmessage=e=>log.push('top:'+e.data);
w.onmessage=e=>log.push('child:'+e.data);
w.postMessage('a1','/'); w.eval("parent.postMessage('a2','*')");
setTimeout(()=>log.push('timer b0'),0); postMessage('b1','*');
postMessage('c1','*'); w.postMessage('c2','*');
w.postMessage('d1','*'); postMessage('d2','*');
setTimeout(()=>log.push('timer e0'),0); w.postMessage('e1','*');
w.setTimeout(()=>log.push('child timer f0'),0); postMessage('f1','*');
w.postMessage('g1','*'); w.setTimeout(()=>log.push('child timer g2'),0); setTimeout(()=>log.push('timer g3'),0);
const before=setTimeout(()=>{},0); postMessage('h1','*'); w.postMessage('h2','*'); log.push('timer ids consecutive:'+(setTimeout(()=>{},0)===before+1));"#;

const CHROME_156: &str = r#"["timer ids consecutive:true","child:a1","top:a2","timer b0","top:b1","top:c1","child:c2","child:d1","top:d2","timer e0","child:e1","child timer f0","top:f1","child:g1","child timer g2","timer g3","top:h1","child:h2"]"#;

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#72: fails until the fix"]
async fn window_messages_and_timers_run_in_posting_order() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://order.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval("globalThis[Symbol.for('ts')].runTimers(200)")
        .await
        .unwrap();
    let log = page.eval_json_expression("log").await.unwrap();
    assert_eq!(log, CHROME_156);
}
