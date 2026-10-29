//! Native form navigation's documented limits (CLAUDE.md "Honest behavior"): a
//! submission the crawler cannot perform throws a TypeError naming the limit instead
//! of silently doing nothing, and leaves the document in place.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

const PROBE: &str = r#"(()=>{
  const out={};
  const attempt=(name,form)=>{document.body.append(form);try{form.submit();out[name]='submitted';}catch(e){out[name]=e.name+': '+e.message;}};
  const make=(attributes)=>{const form=document.createElement('form');for(const [k,v] of Object.entries(attributes))form.setAttribute(k,v);const input=document.createElement('input');input.name='q';input.value='v';form.append(input);return form;};
  attempt('mailto',make({action:'mailto:someone@example.test',method:'post'}));
  attempt('javascript',make({action:'javascript:void 0'}));
  attempt('unresolvable',make({action:'http://[::1'}));
  attempt('blank target',make({action:'/next',target:'_blank'}));
  const frame=document.createElement('iframe');document.body.append(frame);
  const child=frame.contentDocument;const childForm=child.createElement('form');childForm.action='https://forms.test/child';child.body.append(childForm);
  try{childForm.submit();out.child='submitted';}catch(e){out.child=e.name+': '+e.message;}
  out.location=location.href;
  return out;
})()"#;

#[tokio::test(flavor = "current_thread")]
async fn unsupported_form_navigations_throw_a_named_type_error() {
    let mut page = PageSession::open("<body>", "https://forms.test/page", "", "", 5000)
        .await
        .unwrap();
    let value: Value =
        serde_json::from_str(&page.eval_json_expression(PROBE).await.unwrap()).unwrap();
    assert_eq!(
        value,
        json!({
            "mailto":"TypeError: Native form submission requires an HTTP(S) action",
            "javascript":"submitted",
            "unresolvable":"TypeError: Native form action cannot be resolved",
            "blank target":"TypeError: Native form submission to another browsing context is not supported",
            "child":"TypeError: Native form navigation from child frames is not supported",
            "location":"https://forms.test/page"
        })
    );
}

/// turbo-surf#100: a `javascript:` action navigates to the javascript: URL, which runs its
/// script in a later task of the same document (Chrome 156: `submit()` returns, the script
/// sees the page's globals, the location is unchanged).
#[tokio::test(flavor = "current_thread")]
async fn javascript_form_action_runs_the_script_like_chrome156() {
    let mut page = PageSession::open("<body>", "https://forms.test/page", "", "", 5000)
        .await
        .unwrap();
    let raw = page
        .eval_json_expression(
            r#"new Promise(resolve=>{
  const log=[];globalThis.hit=(x)=>log.push('ran:'+x);
  const f=document.createElement('form');f.action="javascript:hit(typeof%20q)";
  const input=document.createElement('input');input.name='q';input.value='v';f.append(input);document.body.append(f);
  let r;try{f.submit();r='submitted';}catch(e){r=e.name+': '+e.message;}
  log.push('sync:'+r+':'+log.length);
  setTimeout(()=>{log.push('href:'+location.href);resolve(log);},200);
})"#,
        )
        .await
        .unwrap();
    page.close();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!([
            "sync:submitted:0",
            "ran:undefined",
            "href:https://forms.test/page"
        ])
    );
}

async fn csp_page(csp: &str, html: &str, probe: &str) -> Value {
    let mut page = PageSession::open_with_response_headers(
        html,
        "https://forms.test/page",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[("Content-Security-Policy".into(), csp.into())],
    )
    .await
    .unwrap();
    let raw = page.eval_json_expression(probe).await.unwrap();
    page.close();
    serde_json::from_str(&raw).unwrap()
}

/// turbo-surf#122: under `require-trusted-types-for 'script'` a `javascript:` form action
/// is blocked without a default policy, and with one runs the policy's `createScript`
/// result (sink "Location href"). Chrome 156 (8078.47, headless, localhost).
#[tokio::test(flavor = "current_thread")]
async fn trusted_types_gate_javascript_form_actions_like_chrome156() {
    let html = "<body><form id=f1 action=\"javascript:void(window.r1=1)\"></form>";
    let blocked = csp_page(
        "require-trusted-types-for 'script'",
        html,
        "new Promise(r=>{f1.submit();setTimeout(()=>r(String(window.r1)),100)})",
    )
    .await;
    assert_eq!(blocked, json!("undefined"));
    let policed = csp_page(
        "require-trusted-types-for 'script'",
        html,
        "new Promise(r=>{let seen=[];trustedTypes.createPolicy('default',{createScript:(s,t,k)=>{seen.push(s,k);return s.replace('r1=1','r1=2')}});f1.submit();setTimeout(()=>r([String(window.r1),seen]),100)})",
    )
    .await;
    assert_eq!(
        policed,
        json!(["2", ["void(window.r1=1)", "Location href"]])
    );
}
