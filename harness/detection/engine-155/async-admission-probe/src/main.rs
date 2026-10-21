//! Deterministic termination during native async admission; no network or browser.
use deno_core::{op2, v8, JsRuntime, OpState, RuntimeOptions};
use deno_error::JsErrorBox;
use std::{cell::RefCell, future::Future, rc::Rc};

#[op2]
async fn op_admission_terminate(state: Rc<RefCell<OpState>>) -> u32 {
    let handle = state.borrow().borrow::<v8::IsolateHandle>().clone();
    handle.terminate_execution();
    tokio::task::yield_now().await;
    7
}

#[op2]
async fn op_admission_reject(state: Rc<RefCell<OpState>>) -> Result<(), JsErrorBox> {
    let handle = state.borrow().borrow::<v8::IsolateHandle>().clone();
    handle.terminate_execution();
    tokio::task::yield_now().await;
    Err(JsErrorBox::generic("interrupted native rejection"))
}

#[op2]
async fn op_reject() -> Result<(), JsErrorBox> {
    tokio::task::yield_now().await;
    Err(JsErrorBox::generic("ordinary native rejection"))
}

#[op2]
async fn op_pending() -> u32 {
    tokio::task::yield_now().await;
    11
}

#[op2]
async fn op_eager() -> u32 {
    13
}

#[op2]
fn op_sync_throw() -> Result<impl Future<Output = ()>, JsErrorBox> {
    if std::hint::black_box(true) {
        return Err(JsErrorBox::generic("admission fixture throw"));
    }
    Ok(async {})
}

deno_core::extension!(
    admission_probe,
    ops = [
        op_admission_terminate,
        op_admission_reject,
        op_reject,
        op_pending,
        op_eager,
        op_sync_throw,
    ]
);

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let rejection = std::env::args().any(|arg| arg == "--characterize-rejection");
    let expect_missing = std::env::args().any(|arg| arg == "--expect-missing");
    let mut rt = JsRuntime::new(RuntimeOptions {
        extensions: vec![admission_probe::init()],
        ..Default::default()
    });
    let handle = rt.v8_isolate().thread_safe_handle();
    rt.op_state().borrow_mut().put(handle);
    if rejection {
        rt.execute_script("caught", "Deno.core.ops.op_reject().catch(()=>{})")
            .unwrap();
        rt.run_event_loop(Default::default())
            .await
            .expect("ordinary caught rejection");
        rt.execute_script("uncaught", "Deno.core.ops.op_reject()")
            .unwrap();
        let ordinary = rt
            .run_event_loop(Default::default())
            .await
            .expect_err("ordinary unhandled error must remain visible")
            .to_string();
        assert!(ordinary.contains("ordinary native rejection"), "{ordinary}");
        assert!(rt
            .execute_script(
                "interrupted rejection",
                "Deno.core.ops.op_admission_reject(); while(true){}"
            )
            .is_err());
        rt.v8_isolate().cancel_terminate_execution();
        let interrupted = rt
            .run_event_loop(Default::default())
            .await
            .expect_err("unobserved interrupted rejection must remain visible")
            .to_string();
        assert!(
            interrupted.contains("interrupted native rejection"),
            "{interrupted}"
        );
        assert!(!interrupted.contains("Missing promise"), "{interrupted}");
        println!("PASS: caught rejection clean; ordinary and interrupted unhandled rejection retain genuine error");
        return;
    }

    rt.execute_script(
        "before",
        r#"
      globalThis.results=[];
      Deno.core.ops.op_pending().then(v=>results.push(v));
      Deno.core.ops.op_eager().then(v=>results.push(v));
      Deno.core.ops.op_sync_throw().catch(e=>results.push(e.message));
    "#,
    )
    .unwrap();
    rt.run_event_loop(Default::default()).await.unwrap();
    assert!(rt
        .execute_script(
            "interrupted",
            "Deno.core.ops.op_admission_terminate(); while(true) {}"
        )
        .is_err());
    rt.v8_isolate().cancel_terminate_execution();
    let resumed = rt.run_event_loop(Default::default()).await;
    if expect_missing {
        let error = resumed
            .expect_err("unpatched SDK must reproduce orphan completion")
            .to_string();
        assert!(
            error.contains("Missing promise"),
            "unexpected failure: {error}"
        );
        println!("PASS: deterministic missing-promise admission failure reproduced");
        return;
    }
    resumed.expect("registered interrupted op completion must remain deliverable");
    rt.execute_script("all-generated-arities", r#"
      globalThis.matrix=[];
      for(let n=0;n<10;n++) {
        const args=Array.from({length:n},(_,i)=>i+1);
        for(const mode of ['pending','eager','throw']) {
          let id;
          const original=function(...actual) {
            id=actual.shift();
            if(!Deno.core.hasPromise(id)) throw Error('native admission preceded registration');
            if(actual.join(',')!==args.join(',')) throw Error('arity arguments changed');
            if(mode==='throw') throw Error('matrix throw');
            if(mode==='eager') return n;
          };
          Object.defineProperty(original,'length',{value:n+1});
          const stub=Deno.core.setUpAsyncStub('matrix'+n,original);
          const promise=stub(...args);
          if(mode==='pending') {
            if(!Deno.core.hasPromise(id)) throw Error('pending reservation missing');
            Deno.core.__eventLoopTick(id,true,n);
          }
          if(Deno.core.hasPromise(id)) throw Error('settled reservation retained');
          promise.then(v=>matrix.push(v===n),e=>matrix.push(mode==='throw'&&e.message==='matrix throw'));
        }
      }
    "#).unwrap();
    rt.run_event_loop(Default::default()).await.unwrap();
    rt.execute_script(
        "matrix-verify",
        "if(matrix.length!==30||matrix.some(v=>!v))throw Error('matrix result mismatch')",
    )
    .unwrap();

    rt.execute_script(
        "after",
        r#"
      Deno.core.ops.op_pending().then(v=>results.push(v));
      Deno.core.ops.op_eager().then(v=>results.push(v));
      Deno.core.ops.op_sync_throw().catch(e=>results.push(e.message));
    "#,
    )
    .unwrap();
    rt.run_event_loop(Default::default()).await.unwrap();
    let value = rt
        .execute_script(
            "verify",
            r#"
      if(results.filter(v=>v===11).length!==2 || results.filter(v=>v===13).length!==2 ||
         results.filter(v=>v==='admission fixture throw').length!==2)
        throw Error('pending/eager/throw result mismatch');
      true
    "#,
        )
        .unwrap();
    deno_core::scope!(scope, rt);
    assert!(v8::Local::new(scope, value).is_true());
    println!("PASS: admission recovery, pending/eager/synchronous throw, no unhandled rejection");
}
