//! A timer set inside a timer task is due no earlier than the timers already due when
//! its parent ran (turbo-surf#118): equal due times fire in creation order, nested
//! zero-delay timers queue behind equal-delay siblings, and nesting past level 5 clamps
//! to 4ms. Expected log: Chrome 156 (156.0.8078.47, headless, --virtual-time-budget)
//! running the same script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const log=[];globalThis.log=log;const L=x=>log.push(x);
setTimeout(()=>{L("a");setTimeout(()=>L("a-inner0"),0);},2);setTimeout(()=>L("b"),2);
setTimeout(()=>{L("c");setTimeout(()=>L("c-inner0"),0);setTimeout(()=>L("c-inner0b"),0);},5);setTimeout(()=>L("d"),5);setTimeout(()=>L("e"),5);
setTimeout(()=>L("f"),10);setTimeout(()=>L("g"),10);
let n=0;const deep=()=>{n++;L("n"+n);if(n<8)setTimeout(deep,0);};setTimeout(deep,20);setTimeout(()=>L("p3"),23);setTimeout(()=>L("p30"),30);"#;

const CHROME_156: &str = r#"["a","b","a-inner0","c","d","e","c-inner0","c-inner0b","f","g","n1","n2","n3","n4","n5","n6","p3","n7","n8","p30"]"#;

#[tokio::test(flavor = "current_thread")]
async fn nested_timers_queue_behind_due_siblings_in_chrome_order() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://order.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_engine("__tsNs.runTimers(200)").await.unwrap();
    let log = page.eval_json_expression("log").await.unwrap();
    assert_eq!(log, CHROME_156);
}
