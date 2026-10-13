//! Host-origin input uses a private embedder capability; guest input stays untrusted.
use serde_json::json;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn host_input_orders_trusted_keyboard_edits_and_honors_cancellation() {
    let mut page = PageSession::open(
        "<body><input id=q></body>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval(r#"
      globalThis.observed=[];const q=document.querySelector('#q');
      for(const name of ['keydown','keypress','beforeinput','input','keyup'])q.addEventListener(name,event=>{
        observed.push([event.type,event.isTrusted,event instanceof UIEvent,event instanceof InputEvent?event.data:event.key,q.value,event.cancelable]);
        if(event.type==='beforeinput'&&event.data==='b')event.preventDefault();
      });
      globalThis.original=q.dispatchEvent;
      q.dispatchEvent=()=>{throw new Error('guest override is not a host dispatch capability');};
    "#).await.unwrap();
    page.input(&json!([{"focus":"#q"},{"type":"ab"}]))
        .await
        .unwrap();
    let rows: serde_json::Value = serde_json::from_str(
        &page
            .eval("globalThis.__RESULT=JSON.stringify(observed)")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 9, "{rows}");
    assert_eq!(rows[3], json!(["input", true, true, "a", "a", false]));
    assert_eq!(rows[7], json!(["beforeinput", true, true, "b", "a", true]));
    for row in rows.as_array().unwrap() {
        assert_eq!(row[1], true);
    }
    let trust = page.eval("observed=[];original.call(document.querySelector('#q'),new KeyboardEvent('keydown',{key:'x'}));globalThis.__RESULT=String(observed[0][1]);").await.unwrap();
    assert_eq!(trust, "false");
    assert_eq!(page.eval("globalThis.__RESULT=String('op_set_host_input_driver'in globalThis||'__hostInput'in globalThis||globalThis[Symbol.for('ts.dc')]!==undefined||'Deno'in globalThis)").await.unwrap(), "false");
}

#[tokio::test(flavor = "current_thread")]
async fn implicit_get_form_submission_uses_the_live_values_and_cancelable_submit_event() {
    let mut page = PageSession::open("<body><form action='/search?discard=1'><input id=q name=q><input type=hidden name=token value=live><input type=checkbox name=excluded></form></body>", "https://example.test/home", "", "", 5000).await.unwrap();
    page.eval("globalThis.submits=[];document.querySelector('form').onsubmit=event=>submits.push([event instanceof SubmitEvent,event.isTrusted,event.submitter]);").await.unwrap();
    page.input(&json!([{"focus":"#q"},{"type":"query"},{"press":"Enter"}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval("globalThis.__RESULT=location.href")
            .await
            .unwrap(),
        "https://example.test/search?q=query&token=live"
    );
    assert_eq!(
        page.eval("globalThis.__RESULT=JSON.stringify(submits)")
            .await
            .unwrap(),
        "[[true,true,null]]"
    );
    page.eval("location.href='https://example.test/home';document.querySelector('form').onsubmit=event=>event.preventDefault()").await.unwrap();
    page.input(&json!([{"press":"Enter"}])).await.unwrap();
    assert_eq!(
        page.eval("globalThis.__RESULT=location.href")
            .await
            .unwrap(),
        "https://example.test/home"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pending_host_input_is_bounded_and_the_session_remains_usable() {
    let mut page = PageSession::open(
        "<body><input id=q></body>",
        "https://budget.test/",
        "",
        "",
        100,
    )
    .await
    .unwrap();
    let error = page
        .input(&json!([{"focus":"#q"},{"wait":1_000_000},{"type":"late"}]))
        .await
        .unwrap_err();
    assert!(
        error.contains("budget") || error.contains("Promise resolution is still pending"),
        "{error}"
    );
    assert_eq!(
        page.eval("globalThis.__RESULT=document.body.tagName")
            .await
            .unwrap(),
        "BODY"
    );
    page.input(&json!([{"type":"ok"}])).await.unwrap();
    assert_eq!(
        page.eval("globalThis.__RESULT=document.querySelector('#q').value")
            .await
            .unwrap(),
        "ok"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn completed_host_input_does_not_wait_for_unrelated_background_timers() {
    let mut page = PageSession::open(
        "<body><button id=start>Start</button><output id=ticks></output><script>document.querySelector('#start').onclick=()=>{globalThis.backgroundTimer=setTimeout(function tick(){document.querySelector('#ticks').textContent=String((globalThis.ticks||0)+1);globalThis.ticks=(globalThis.ticks||0)+1;globalThis.backgroundTimer=setTimeout(tick,0)},0);};</script></body>",
        "https://budget.test/",
        "",
        "",
        500,
    ).await.unwrap();
    let started = std::time::Instant::now();
    page.input(&json!([{"click":"#start"}])).await.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_millis(300));
    assert_eq!(page.eval("clearTimeout(backgroundTimer);globalThis.__RESULT=document.querySelector('#start').tagName").await.unwrap(), "BUTTON");
}

#[tokio::test(flavor = "current_thread")]
async fn asynchronous_host_input_stops_pumping_when_its_own_edits_complete() {
    let mut page = PageSession::open(
        "<body><input id=q><script>globalThis.ticks=0;globalThis.releases=[];const q=document.querySelector('#q');q.oninput=()=>{if(globalThis.backgroundTimer==null)backgroundTimer=setTimeout(function tick(){ticks++;backgroundTimer=setTimeout(tick,10)},10);};q.onkeyup=event=>releases.push(event.key);</script></body>",
        "https://budget.test/", "", "", 500,
    ).await.unwrap();
    page.input(&json!([{"focus":"#q"},{"type":"ok"}]))
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&page.eval("clearTimeout(backgroundTimer);globalThis.__RESULT=JSON.stringify([document.querySelector('#q').value,releases,ticks]);").await.unwrap()).unwrap();
    assert_eq!(result[0], "ok");
    assert_eq!(result[1], json!(["o", "k"]));
    assert!(result[2].as_u64().unwrap() < 100, "{result}");
}

#[tokio::test(flavor = "current_thread")]
async fn synchronous_runaway_input_is_bounded_and_the_session_remains_usable() {
    let mut page = PageSession::open(
        "<body><input id=q><script>document.querySelector('#q').onkeydown=()=>{while(true){}};</script></body>",
        "https://budget.test/", "", "", 100,
    ).await.unwrap();
    let started = std::time::Instant::now();
    assert!(page
        .input(&json!([{"focus":"#q"},{"type":"late"}]))
        .await
        .is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    let partial = page.eval("document.querySelector('#q').onkeydown=null;globalThis.__RESULT=document.querySelector('#q').value;document.querySelector('#q').value=''").await.unwrap();
    assert!(
        partial.len() <= 1,
        "runaway input continued after its budget: {partial}"
    );
    page.input(&json!([{"type":"ok"}])).await.unwrap();
    assert_eq!(
        page.eval("globalThis.__RESULT=document.querySelector('#q').value")
            .await
            .unwrap(),
        "ok"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn terminated_guest_initialization_retains_atomic_trusted_input_and_crypto_readiness() {
    let mut page = PageSession::open_with_init_script(
        "<body><input id=q></body>",
        "https://budget.test/",
        "",
        "",
        100,
        "globalThis.beforeRunaway='entered';document.querySelector('#q').oninput=event=>globalThis.inputTrust=event.isTrusted;while(true){}",
    ).await.unwrap();
    assert!(page.hydration_diagnostics().budget_exceeded);
    assert_eq!(page.eval("globalThis.__RESULT=JSON.stringify([beforeRunaway,typeof Deno,'Deno'in globalThis,globalThis[Symbol.for('ts.dc')]===undefined,typeof crypto.subtle.digest])").await.unwrap(), "[\"entered\",\"undefined\",false,true,\"function\"]");
    page.input(&json!([{"focus":"#q"},{"type":"ok"}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval(
            "globalThis.__RESULT=JSON.stringify([document.querySelector('#q').value,inputTrust])"
        )
        .await
        .unwrap(),
        "[\"ok\",true]"
    );
}
