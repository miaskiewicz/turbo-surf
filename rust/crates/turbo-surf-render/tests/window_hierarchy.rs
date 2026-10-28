//! Window hierarchy from script: frameElement / top / parent / length / indexed
//! access through nested frames, postMessage target-origin matching ("/" is the
//! sender's origin, a different origin drops the message, an invalid one throws), and
//! a removed frame's window losing its parent. Expected: Chrome 156 (156.0.8078.4,
//! headless) running the same script on a localhost page (the message origin is the
//! page's own origin there too).
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"(async()=>{const log=[];const tick=()=>new Promise(r=>setTimeout(r,30));
const f=document.createElement('iframe');document.body.append(f);const w=f.contentWindow;
log.push(w.frameElement===f, w.top===window, w.parent===window, window.frameElement, w.length, window.length, window[0]===w);
const g=document.createElement('iframe');f.contentDocument.body.append(g);log.push(g.contentWindow.top===window, g.contentWindow.parent===w, w.length);
globalThis.sources=[];window.onmessage=e=>{sources.push(e.source===w);log.push('top got:'+e.data+':'+e.origin)};
w.postMessage('wrong','https://other.test');w.postMessage('slash','/');
w.onmessage=e=>log.push('child got:'+e.data);
w.eval("parent.postMessage('up','*')");
await tick();
try{w.postMessage('bad','not a url')}catch(e){log.push(e.name)}
const fe=Object.getOwnPropertyDescriptor(Window.prototype,'frameElement')||Object.getOwnPropertyDescriptor(window,'frameElement');
try{fe.get.call({})}catch(e){log.push('fe:'+e.name)}
f.remove();log.push(w.parent===null, w.top===null, w.frameElement);
return log;})()"#;

const CHROME_156: &str = r#"[true,true,true,null,0,1,true,true,true,1,"child got:slash","top got:up:https://windows.test","SyntaxError","fe:TypeError",true,true,null]"#;

#[tokio::test(flavor = "current_thread")]
async fn window_hierarchy_and_message_targets_match_chrome() {
    let mut page = PageSession::open("<body>", "https://windows.test/", "", "", 5000)
        .await
        .unwrap();
    let mut log: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(SCRIPT).await.unwrap()).unwrap();
    let mut chrome: serde_json::Value = serde_json::from_str(CHROME_156).unwrap();
    // turbo-surf#72: cross-window message order; compare the two deliveries as a set.
    for entries in [&mut log, &mut chrome] {
        entries.as_array_mut().unwrap()[10..12].sort_by_key(|entry| entry.to_string());
    }
    assert_eq!(log, chrome);
    // Chrome: the message source is the sender's WindowProxy (true); surf hands the
    // parent a different object on both engines (turbo-surf#72).
    let sources = page.eval_json_expression("sources").await.unwrap();
    assert_eq!(sources, "[false]");
}
