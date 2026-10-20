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
async fn implicit_form_submission_uses_live_values_validation_and_trusted_cancellation() {
    let mut page = PageSession::open("<body><form action='/search'><input id=q name=q required><input type=hidden name=token value=live></form></body>", "https://example.test/home", "", "", 5000).await.unwrap();
    page.eval("globalThis.submits=[];document.querySelector('form').onsubmit=event=>{event.preventDefault();submits.push([event instanceof SubmitEvent,event.isTrusted,event.submitter,document.querySelector('#q').value]);};").await.unwrap();
    page.input(&json!([{"focus":"#q"},{"press":"Enter"}]))
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression("submits").await.unwrap(), "[]");
    page.input(&json!([{"type":"query"},{"press":"Enter"}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval_json_expression("submits").await.unwrap(),
        "[[true,true,null,\"query\"]]"
    );
    assert_eq!(page.navigation_snapshot().0, 0);
    assert_eq!(page.navigation_snapshot().1, "https://example.test/home");
    page.eval("HTMLFormElement.prototype.requestSubmit=()=>{throw Error('public replacement');};Object.defineProperty(document.querySelector('#q'),'form',{get(){throw Error('public form getter');}});").await.unwrap();
    page.input(&json!([{"press":"Enter"}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression("submits.length").await.unwrap(),
        "2"
    );
    page.close();
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

#[tokio::test(flavor = "current_thread")]
async fn json_collectors_await_values_without_page_serializers_or_result_slots() {
    let mut page = PageSession::open("<body></body>", "https://collector.test/", "", "", 1000)
        .await
        .unwrap();
    page.eval_expression(r#"(() => {
      globalThis.collectorHooks=0;
      JSON.stringify=function(){collectorHooks++;throw new Error('page serializer')};
      Object.defineProperty(globalThis,'__RESULT',{get(){collectorHooks++;throw new Error('page result slot')},configurable:true});
      return true;
    })()"#).await.unwrap();
    let raw = page
        .eval_json_expression("Promise.resolve({rows:[1,false,null],nested:{native:true}})")
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"rows":[1,false,null],"nested":{"native":true}})
    );
    assert_eq!(page.eval_json_expression("false").await.unwrap(), "false");
    assert_eq!(page.eval_json_expression("0").await.unwrap(), "0");
    assert_eq!(page.eval_json_expression("null").await.unwrap(), "null");
    assert_eq!(page.eval_expression("collectorHooks").await.unwrap(), "0");
    let error = page
        .eval_json_expression("({get value(){throw new Error('authored collector getter')}})")
        .await
        .unwrap_err();
    assert!(error.contains("authored collector getter"), "{error}");
    assert!(page
        .eval_json_expression("undefined")
        .await
        .unwrap_err()
        .contains("JSON serializable"));
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_frame_creation_never_exposes_bootstrap_host_capabilities() {
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
        .eval_expression(
            r#"(() => {
      globalThis.attemptedFrames=[];
      globalThis.retainedFrames=[];
      while(true){
        const frame=document.createElement('iframe');
        // Retain the node even when its first bootstrap exhausts the budget.
        attemptedFrames.push(frame);
        document.body.appendChild(frame);
        retainedFrames.push(frame.contentWindow);
        frame.srcdoc='<body>new document</body>';
      }
    })()"#,
        )
        .await
        .unwrap_err();
    assert!(error.contains("budget"), "{error}");
    let result: serde_json::Value = serde_json::from_str(
        &page
            .eval_expression(
                r#"JSON.stringify((()=>{
      const count=attemptedFrames.length,available=window.frames.length;
      let inspected=0,retired=0,unavailable=count-available,exposed=false;
      const inspect=frame=>{
        if(frame===null||frame===undefined)return 'unavailable';
        try {
          exposed=exposed || 'Deno' in frame || '__bootstrap' in frame ||
            frame[Symbol.for('ts.dc')]!==undefined || 'op_set_host_input_driver' in frame;
          return frame.closed?'retired':'inspected';
        } catch(error){if(error.name==='SecurityError')return 'retired';throw error;}
      };
      // contentWindow could start a new bootstrap; inspect only published realms.
      for(let index=0;index<available;index++){
        const state=inspect(window.frames[index]);
        if(state==='inspected')inspected++;else if(state==='retired')retired++;else unavailable++;
      }
      for(const frame of retainedFrames)inspect(frame);
      return {count,inspected,retired,unavailable,exposed,
        parent:'Deno' in globalThis || '__bootstrap' in globalThis};
    })())"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(result["count"].as_u64().unwrap() > 0, "{result}");
    assert_eq!(
        result["count"].as_u64().unwrap(),
        ["inspected", "retired", "unavailable"]
            .iter()
            .map(|name| result[name].as_u64().unwrap())
            .sum::<u64>(),
        "{result}"
    );
    assert_eq!(result["exposed"], false, "{result}");
    assert_eq!(result["parent"], false, "{result}");
    page.input(&json!([{"focus":"#q"},{"type":"ok"}]))
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression("document.querySelector('#q').value")
            .await
            .unwrap(),
        "ok"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn host_edits_use_captured_document_and_control_accessors() {
    let mut page = PageSession::open(
        "<input id=q><textarea id=t></textarea><input id=disabled><textarea id=readonly readonly></textarea>",
        "https://example.test/", "", "", 5000,
    ).await.unwrap();
    page.eval(r#"
      globalThis.poisonCalls=[];globalThis.bodyEdits=0;
      document.body.addEventListener('input',event=>{if(event.target===document.body)bodyEdits++});
      globalThis.realValues=[HTMLInputElement,HTMLTextAreaElement].map(C=>Object.getOwnPropertyDescriptor(C.prototype,'value').get);
      for(const id of ['q','t','disabled','readonly']) {
        const control=document.getElementById(id);
        for(const key of ['value','disabled','readOnly'])Object.defineProperty(control,key,{configurable:true,get(){poisonCalls.push(id+'.'+key);throw Error('guest control getter')}});
      }
      Object.defineProperty(document,'activeElement',{configurable:true,get(){poisonCalls.push('document.activeElement');throw Error('guest activeElement')}});
    "#).await.unwrap();
    page.input(&json!([
        {"focus":"#q"},{"type":"ab"},
        {"focus":"#t"},{"type":"cd"},
        {"focus":"#readonly"},{"type":"bad"},
        {"focus":"#disabled"},
    ]))
    .await
    .unwrap();
    page.eval("document.getElementById('disabled').setAttribute('disabled','')")
        .await
        .unwrap();
    page.input(&json!([{"type":"bad"}])).await.unwrap();
    page.input(&json!([{"focus":"body"},{"type":"ignored"}]))
        .await
        .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&page.eval_json_expression(
        "({poisonCalls,bodyEdits,values:['q','t','disabled','readonly'].map((id,i)=>realValues[i===1||i===3?1:0].call(document.getElementById(id)))})"
    ).await.unwrap()).unwrap();
    assert_eq!(actual["poisonCalls"], json!([]));
    assert_eq!(actual["bodyEdits"], 0);
    assert_eq!(actual["values"], json!(["ab", "cd", "", ""]));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn script_navigation_captures_source_activation_at_admission() {
    for trigger in ["script", "click", "keydown"] {
        let mut page = PageSession::open(
            "<button id=go>go</button><input id=q>",
            "https://example.test/start",
            "",
            "",
            5000,
        )
        .await
        .unwrap();
        match trigger {
            "script" => {
                page.eval("location.assign('/next')").await.unwrap();
            }
            "click" => {
                page.eval("document.querySelector('#go').addEventListener('pointerdown',()=>location.assign('/next'))").await.unwrap();
                page.input(&json!([{"click":"#go"}])).await.unwrap();
            }
            "keydown" => {
                page.eval("document.querySelector('#q').addEventListener('keydown',()=>location.href='/next')").await.unwrap();
                page.input(&json!([{"focus":"#q"},{"press":"x"}]))
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            page.script_nav_activation_snapshot(),
            Some(("https://example.test/next".into(), trigger != "script"))
        );
        if trigger == "click" {
            tokio::time::sleep(std::time::Duration::from_millis(5100)).await;
            // The earlier admitted navigation keeps its actual source state.
            assert_eq!(
                page.script_nav_activation_snapshot(),
                Some(("https://example.test/next".into(), true))
            );
            page.eval("location.replace('/later')").await.unwrap();
            assert_eq!(
                page.script_nav_activation_snapshot(),
                Some(("https://example.test/later".into(), false))
            );
        }
        page.close();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn clicks_resolve_current_target_center_without_a_prior_move() {
    let mut page = PageSession::open(
        "<body style='margin:0'><button id=q style='position:absolute;left:20px;top:30px;width:80px;height:40px'>Click</button></body>",
        "https://example.test/", "", "", 5000,
    ).await.unwrap();
    page.eval(r#"
      globalThis.positions=[];globalThis.expected=[];
      const q=document.querySelector('#q'),rect=Element.prototype.getBoundingClientRect;
      globalThis.recordCenter=()=>{const r=rect.call(q);expected.push([r.left+r.width/2,r.top+r.height/2]);};
      recordCenter();q.onclick=e=>positions.push([e.clientX,e.clientY,e.isTrusted]);
      q.getBoundingClientRect=()=>{throw Error('guest rectangle override');};
    "#).await.unwrap();
    page.input(&json!([{"click":"#q"}])).await.unwrap();
    page.input(&json!([{"move":{"selector":"#q"}}]))
        .await
        .unwrap();
    page.eval("document.querySelector('#q').style.left='170px';document.querySelector('#q').style.top='90px';recordCenter();").await.unwrap();
    page.input(&json!([{"click":"#q"}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression("positions").await.unwrap(),
        page.eval_json_expression("expected.map(p=>[...p,true])")
            .await
            .unwrap()
    );
    assert_eq!(
        page.eval_json_expression(
            "positions.length===2&&positions[0][0]>0&&positions[1][0]>positions[0][0]"
        )
        .await
        .unwrap(),
        "true"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn typing_replaces_live_control_selections_and_preserves_cancellation() {
    let mut page = PageSession::open(
        "<body><input id=q><textarea id=t></textarea></body>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval(r#"
      globalThis.edits=[];globalThis.readControl=[];
      for(const [id,C] of [['q',HTMLInputElement],['t',HTMLTextAreaElement]]){
        const q=document.getElementById(id),p=C.prototype;
        const value=Object.getOwnPropertyDescriptor(p,'value');
        const start=Object.getOwnPropertyDescriptor(p,'selectionStart').get;
        const end=Object.getOwnPropertyDescriptor(p,'selectionEnd').get;
        const select=p.setSelectionRange;
        value.set.call(q,'abcd');select.call(q,1,3,'backward');
        readControl.push(()=>[value.get.call(q),start.call(q),end.call(q)]);
        q.onbeforeinput=e=>{
          edits.push([id,e.type,e.data,e.isTrusted]);
          if(e.data==='!')e.preventDefault();
          if(e.data==='Z')select.call(q,0,1);
        };
        q.oninput=e=>edits.push([id,e.type,e.data,e.isTrusted]);
        for(const key of ['value','selectionStart','selectionEnd'])Object.defineProperty(q,key,{get(){throw Error('guest control accessor');},set(){throw Error('guest control setter');},configurable:true});
        q.setSelectionRange=()=>{throw Error('guest selection method');};
      }
    "#).await.unwrap();
    for selector in ["#q", "#t"] {
        page.input(&json!([{"focus":selector},{"type":"X!YZ"}]))
            .await
            .unwrap();
    }
    assert_eq!(
        page.eval_json_expression("readControl.map(read=>read())")
            .await
            .unwrap(),
        "[[\"ZXYd\",1,1],[\"ZXYd\",1,1]]"
    );
    assert_eq!(page.eval_json_expression("edits.filter(e=>e[1]==='input').map(e=>[e[0],e[2],e[3]])").await.unwrap(), "[[\"q\",\"X\",true],[\"q\",\"Y\",true],[\"q\",\"Z\",true],[\"t\",\"X\",true],[\"t\",\"Y\",true],[\"t\",\"Z\",true]]");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn input_guard_preserves_selection_utf16_and_multiple_typing_chunks() {
    let mut page = PageSession::open(
        "<body><textarea id=q>A😀BC</textarea></body>",
        "https://guard.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.input(&json!([{"focus":"#q"}])).await.unwrap();
    page.eval("document.querySelector('#q').setSelectionRange(1,3)")
        .await
        .unwrap();
    let globals = page
        .eval_json_expression("Object.getOwnPropertyNames(globalThis)")
        .await
        .unwrap();
    page.input(&json!([
        {"inputGuard":{"begin":"Ω😀","selector":"#q"}},
        {"type":"Ω😀"}, {"inputGuard":"verify"},
        {"inputGuard":{"begin":"!","selector":"#q"}},
        {"type":"!"}, {"inputGuard":"verify"}, {"inputGuard":"clear"}
    ]))
    .await
    .unwrap();
    assert_eq!(
        page.eval_json_expression("document.querySelector('#q').value")
            .await
            .unwrap(),
        "\"AΩ😀!BC\""
    );
    assert_eq!(
        page.eval_json_expression("Object.getOwnPropertyNames(globalThis)")
            .await
            .unwrap(),
        globals
    );
    let error = page
        .input(&json!([{"inputGuard":"verify"}]))
        .await
        .unwrap_err();
    assert!(error.contains("No retained input control"), "{error}");
}

async fn guarded_input_page(attributes: &str, listener: &str) -> PageSession {
    let mut page = PageSession::open(
        &format!("<body><form><input id=q name=q {attributes}></form><input id=other></body>"),
        "https://guard.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval(&format!("globalThis.submits=0;document.querySelector('form').onsubmit=e=>{{e.preventDefault();submits++}};{listener}"))
        .await
        .unwrap();
    page.input(&json!([{"focus":"#q"}])).await.unwrap();
    page
}

async fn rejected_guarded_query(page: &mut PageSession, text: &str) -> String {
    let error = page
        .input(&json!([
            {"inputGuard":{"begin":text,"selector":"#q"}}, {"type":text},
            {"inputGuard":"verify"}, {"inputGuard":"clear"}, {"press":"Enter"}
        ]))
        .await
        .unwrap_err();
    assert!(error.contains("expected focused query"), "{error}");
    assert_eq!(page.eval_json_expression("submits").await.unwrap(), "0");
    let retired = page
        .input(&json!([{"inputGuard":"verify"}]))
        .await
        .unwrap_err();
    assert!(retired.contains("No retained input control"), "{retired}");
    error
}

#[tokio::test(flavor = "current_thread")]
async fn input_guard_rejects_truncated_typing_without_repair_or_submission() {
    let mut page = guarded_input_page("", "document.querySelector('#q').oninput=event=>{event.target.value=event.target.value.slice(0,2)};").await;
    rejected_guarded_query(&mut page, "query").await;
    assert_eq!(
        page.eval_json_expression("document.querySelector('#q').value")
            .await
            .unwrap(),
        "\"qu\""
    );
}

#[tokio::test(flavor = "current_thread")]
async fn input_guard_rejects_focus_loss_without_refocusing_or_submission() {
    let mut page = guarded_input_page(
        "",
        "document.querySelector('#q').oninput=()=>document.querySelector('#other').focus();",
    )
    .await;
    let error = rejected_guarded_query(&mut page, "x").await;
    assert!(error.contains(r#""focused":false"#), "{error}");
    assert_eq!(
        page.eval_json_expression("document.activeElement.id")
            .await
            .unwrap(),
        "\"other\""
    );
    assert_eq!(
        page.eval_json_expression("document.querySelector('#q').value")
            .await
            .unwrap(),
        "\"x\""
    );
}

#[tokio::test(flavor = "current_thread")]
async fn input_guard_rejects_a_focused_same_selector_replacement_with_matching_value() {
    let mut page = guarded_input_page("", "document.querySelector('#q').oninput=event=>{const old=event.target;const replacement=old.cloneNode();replacement.value=old.value;old.replaceWith(replacement);replacement.focus()};").await;
    let error = rejected_guarded_query(&mut page, "x").await;
    assert!(error.contains(r#""connected":false"#), "{error}");
    assert_eq!(
        page.eval_json_expression("[document.activeElement.id,document.querySelector('#q').value]")
            .await
            .unwrap(),
        "[\"q\",\"x\"]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn input_guard_requires_the_intended_control_before_typing() {
    let mut page = guarded_input_page("", "").await;
    page.input(&json!([{"focus":"#other"}])).await.unwrap();
    let error = page
        .input(&json!([
            {"inputGuard":{"begin":"x","selector":"#q"}}, {"type":"x"}
        ]))
        .await
        .unwrap_err();
    assert!(
        error.contains("intended connected editable control"),
        "{error}"
    );
    assert_eq!(
        page.eval_json_expression(
            "[document.querySelector('#q').value,document.querySelector('#other').value]"
        )
        .await
        .unwrap(),
        "[\"\",\"\"]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn input_guard_rechecks_before_enter_and_releases_before_successful_submission() {
    let mut page = guarded_input_page("", "").await;
    page.input(&json!([
        {"inputGuard":{"begin":"query","selector":"#q"}}, {"type":"query"},
        {"inputGuard":"verify"}
    ]))
    .await
    .unwrap();
    page.eval("document.querySelector('#q').value='truncated'")
        .await
        .unwrap();
    let error = page
        .input(&json!([
            {"inputGuard":"verify"}, {"inputGuard":"clear"}, {"press":"Enter"}
        ]))
        .await
        .unwrap_err();
    assert!(error.contains("expected focused query"), "{error}");
    assert_eq!(page.eval_json_expression("submits").await.unwrap(), "0");
    page.input(&json!([
        {"inputGuard":{"begin":"!","selector":"#q"}}, {"type":"!"},
        {"inputGuard":"verify"}, {"inputGuard":"clear"}, {"press":"Enter"}
    ]))
    .await
    .unwrap();
    assert_eq!(page.eval_json_expression("submits").await.unwrap(), "1");
    let released = page
        .input(&json!([{"inputGuard":"verify"}]))
        .await
        .unwrap_err();
    assert!(released.contains("No retained input control"), "{released}");
}

#[tokio::test(flavor = "current_thread")]
async fn input_budget_cancellation_releases_guard_and_prevents_delayed_type_or_enter() {
    let mut page = PageSession::open_with_document_response_and_options(
        "<form><input id=q name=q></form><script>window.submits=0;document.querySelector('form').onsubmit=e=>{e.preventDefault();submits++}</script>",
        "https://guard.test/", "", "", 100, "",
        turbo_surf_core::request_context::ClientHints::default(), &[],
        turbo_surf_render::BrowserStorage::default(),
        turbo_surf_render::DocumentResponseMetadata::default(), None,
        turbo_surf_render::PageSessionOptions {
            window_timer_mode: turbo_surf_render::WindowTimerMode::RealTime,
            ..Default::default()
        },
    ).await.unwrap();
    page.input(&json!([{"focus":"#q"}])).await.unwrap();
    let error = page
        .input(&json!([
            {"inputGuard":{"begin":"late","selector":"#q"}}, {"wait":250},
            {"type":"late"}, {"press":"Enter"}
        ]))
        .await
        .unwrap_err();
    assert!(error.contains("budget"), "{error}");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    page.eval("void 0").await.unwrap();
    assert_eq!(
        page.eval_json_expression("[submits,document.querySelector('#q').value]")
            .await
            .unwrap(),
        "[0,\"\"]"
    );
    let released = page
        .input(&json!([{"inputGuard":"verify"}]))
        .await
        .unwrap_err();
    assert!(released.contains("No retained input control"), "{released}");
    page.eval("window.recoveredKeys=[];document.querySelector('#q').onkeydown=e=>recoveredKeys.push([e.key,e.isTrusted])")
        .await
        .unwrap();
    page.input(&json!([{"press":"Escape"}])).await.unwrap();
    assert_eq!(
        page.eval_json_expression("[submits,document.querySelector('#q').value,recoveredKeys]")
            .await
            .unwrap(),
        "[0,\"\",[[\"Escape\",true]]]"
    );
}
