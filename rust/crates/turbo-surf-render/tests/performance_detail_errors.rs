//! An uncloneable User Timing `detail` throws Chrome's DataCloneError with the
//! calling operation's prefix (turbo-surf#78): `performance.mark`,
//! `performance.measure` and `new PerformanceMark`. Expected values: Chrome 156
//! (156.0.8078.47, headless) running the same script on a localhost page.
use serde_json::Value;
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"const out=[];const T=f=>{try{f();out.push('ok')}catch(e){out.push(e.name+': '+e.message)}};
T(()=>performance.mark('x',{detail:()=>{}}));
T(()=>performance.measure('y',{detail:()=>{},start:0}));
T(()=>new PerformanceMark('z',{detail:()=>{}}));
T(()=>performance.mark('x2',{detail:Symbol('s')}));
T(()=>performance.measure('y2',{detail:document.body,start:0}));
T(()=>new PerformanceMark('z2',{detail:{a:new WeakMap()}}));
T(()=>{const m=performance.mark('ok',{detail:{a:[1,{b:2}]}});if(m.detail.a[1].b!==2)throw new Error('bad')});
globalThis.out=out;"#;

const CHROME_156: &str = r#"["DataCloneError: Failed to execute 'mark' on 'Performance': ()=>{} could not be cloned.","DataCloneError: Failed to execute 'measure' on 'Performance': ()=>{} could not be cloned.","DataCloneError: Failed to construct 'PerformanceMark': ()=>{} could not be cloned.","DataCloneError: Failed to execute 'mark' on 'Performance': Symbol(s) could not be cloned.","DataCloneError: Failed to execute 'measure' on 'Performance': HTMLBodyElement object could not be cloned.","DataCloneError: Failed to construct 'PerformanceMark': #<WeakMap> could not be cloned.","ok"]"#;

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#78: fails until detail clones carry the caller's prefix"]
async fn performance_detail_clone_errors_match_chrome156() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://timing.test/", "", "", 5000)
        .await
        .unwrap();
    let got: Value =
        serde_json::from_str(&page.eval_json_expression("out").await.unwrap()).unwrap();
    assert_eq!(got, serde_json::from_str::<Value>(CHROME_156).unwrap());
}
