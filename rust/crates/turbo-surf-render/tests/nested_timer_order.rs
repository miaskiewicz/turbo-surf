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

#[tokio::test(flavor = "current_thread")]
async fn nested_clamp_uses_task_deadlines_after_a_host_pause() {
    let mut page = PageSession::open("<body>", "https://order.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, &format!("{SCRIPT};return 'armed'"))
        .unwrap();
    let mut reached = false;
    for _ in 0..32 {
        assert_eq!(
            page.read_target_engine(root, "return __tsNs.runTimers(1)")
                .unwrap(),
            "1"
        );
        reached = page
            .read_target_engine(root, "return log[log.length-1]==='n6'")
            .unwrap()
            == "true";
        if reached {
            break;
        }
    }
    assert!(
        reached,
        "the controlled pause must precede the first clamped timer"
    );
    // Host work between virtual tasks must not become time spent inside the
    // agent: n7's nested deadline remains before the already-queued p30.
    std::thread::sleep(std::time::Duration::from_millis(40));
    page.read_target_engine(root, "return __tsNs.runTimers(200)")
        .unwrap();
    assert_eq!(page.eval_json_expression("log").await.unwrap(), CHROME_156);
}
