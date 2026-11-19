//! Explicit automation policy, distinct from reduced-motion media or screenshot capture.
use serde_json::{json, Value};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::{BrowserStorage, PageSession};

const EFFECTS: &str = r#"<!doctype html><style>
@keyframes rotate{to{transform:rotate(360deg)}}
@keyframes dash{to{stroke-dashoffset:100px}}
@keyframes fade{from{opacity:0}to{opacity:1}}
#target{width:40px;height:20px;animation:rotate 1s infinite}
#fade{opacity:0.6;animation:fade 100s both}
#dash{stroke-dashoffset:7px;animation:dash 1s infinite}
#transition{opacity:.2;transition:opacity 100s}
</style><body><div id=target></div><div id=fade></div><svg><circle id=dash /></svg><div id=transition></div>"#;

async fn open(policy: &str, html: &str) -> PageSession {
    let storage = BrowserStorage::default();
    storage
        .configure_emulation(&json!({"animations": policy}).to_string())
        .unwrap();
    open_storage(storage, html).await
}

async fn open_storage(storage: BrowserStorage, html: &str) -> PageSession {
    PageSession::open_with_browsing_state(
        html,
        "http://127.0.0.1/animation-policy",
        "",
        "",
        5000,
        "",
        ClientHints::default(),
        &[],
        storage,
    )
    .await
    .unwrap()
}

async fn read(page: &mut PageSession, expression: &str) -> Value {
    serde_json::from_str(&page.eval_engine_json_expression(expression).await.unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_css_effects_use_underlying_geometry_and_keep_waapi_surface() {
    let mut page = open("disabled", EFFECTS).await;
    let actual = read(&mut page, r#"(() => {
      const target=document.getElementById('target');
      const style=getComputedStyle(target), box=target.getBoundingClientRect();
      return [style.transform,box.width,box.height,getComputedStyle(document.getElementById('fade')).opacity,
        getComputedStyle(document.getElementById('dash')).strokeDashoffset,
        typeof target.animate,typeof document.timeline,document.getAnimations().length];
    })()"#).await;
    assert_eq!(
        actual,
        json!(["none", 40, 20, "0.6", "7px", "function", "object", 3])
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_policy_covers_later_css_and_transition_changes() {
    let mut page = open("disabled", EFFECTS).await;
    let actual = read(&mut page, r#"(async () => {
      const transition=document.getElementById('transition');
      getComputedStyle(transition).opacity;
      const events=[];transition.addEventListener('transitionrun',()=>events.push('run'));
      transition.style.opacity='0.9';
      getComputedStyle(transition).opacity;
      const child=document.createElement('div');child.style.cssText='width:12px;height:13px;animation:rotate 1s infinite';document.body.append(child);
      await new Promise(resolve=>requestAnimationFrame(resolve));
      return [getComputedStyle(transition).opacity,child.getBoundingClientRect().width,
        getComputedStyle(child).transform,document.getAnimations().length>0,events.includes('run')];
    })()"#).await;
    assert_eq!(actual, json!(["0.9", 12, "none", true, true]));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_policy_preserves_async_application_work_and_frame_callbacks() {
    let mut page = open("disabled", EFFECTS).await;
    let actual = read(
        &mut page,
        r#"(async()=>{
      const steps=[];await new Promise(resolve=>setTimeout(()=>{steps.push('login');resolve()},0));
      await new Promise(resolve=>requestAnimationFrame(()=>{steps.push('frame');resolve()}));
      await Promise.resolve().then(()=>steps.push('logout'));
      return [steps,document.getElementById('target').getBoundingClientRect().width];
    })()"#,
    )
    .await;
    assert_eq!(actual, json!([["login", "frame", "logout"], 40]));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn enabled_context_keeps_css_effects_independent_of_disabled_context() {
    let mut disabled = open("disabled", EFFECTS).await;
    let mut enabled = open("enabled", EFFECTS).await;
    assert_eq!(
        read(
            &mut disabled,
            "getComputedStyle(document.getElementById('target')).transform"
        )
        .await,
        json!("none")
    );
    assert!(
        read(&mut enabled, "document.getAnimations().length")
            .await
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        read(
            &mut disabled,
            "getComputedStyle(document.getElementById('target')).transform"
        )
        .await,
        json!("none")
    );
    enabled.close();
    disabled.close();
}

#[test]
fn animation_policy_rejects_unknown_modes_before_session_configuration() {
    let storage = BrowserStorage::default();
    assert!(storage
        .configure_emulation(r#"{"animations":"fast-forward"}"#)
        .is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_policy_preserves_real_waapi_completion_without_effect_presentation() {
    let mut page = open("disabled", "<!doctype html><div id=target style='opacity:0.6;transform:none;width:40px;height:20px'></div>").await;
    let actual=read(&mut page,r#"(async()=>{
      const target=document.getElementById('target');
      const animation=target.animate({opacity:[0,1],transform:['rotate(0deg)','rotate(90deg)']},{duration:64,fill:'both'});
      await animation.ready;
      const during=[getComputedStyle(target).opacity,getComputedStyle(target).transform,target.getBoundingClientRect().width];
      const finished=await animation.finished;
      return [during,animation.playState,finished===animation,getComputedStyle(target).opacity,target.getBoundingClientRect().width];
    })()"#).await;
    assert_eq!(
        actual,
        json!([["0.6", "none", 40], "finished", true, "0.6", 40])
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_policy_inherits_into_later_child_frames() {
    let mut page = open("disabled", "<!doctype html><body>").await;
    let actual=read(&mut page,r#"(async()=>{
      const frame=document.createElement('iframe');
      frame.srcdoc='<style>@keyframes spin{to{transform:rotate(90deg)}}#target{width:40px;height:20px;transform:none;animation:spin 1s infinite}</style><div id=target></div>';
      await new Promise(resolve=>{frame.onload=resolve;document.body.append(frame)});
      const child=frame.contentWindow,target=child.document.getElementById('target');
      return [child.getComputedStyle(target).transform,target.getBoundingClientRect().width,typeof target.animate,child.document.getAnimations().length];
    })()"#).await;
    assert_eq!(actual, json!(["none", 40, "function", 1]));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_policy_admits_paused_filling_and_multiple_named_effects() {
    let mut page = open("disabled", EFFECTS).await;
    let actual = read(
        &mut page,
        r#"(() => {
      const target=document.getElementById('target');
      return ['rotate 100s paused both','rotate 1s infinite','rotate 1ms forwards',
        'rotate 1s infinite, fade 100s both'].map(animation=>{
          target.style.animation=animation;
          return [target.getBoundingClientRect().width,getComputedStyle(target).transform];
        });
    })()"#,
    )
    .await;
    assert_eq!(
        actual,
        json!([[40, "none"], [40, "none"], [40, "none"], [40, "none"]])
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_policy_uses_private_proof_despite_guest_animation_getters() {
    let mut page = open("disabled", EFFECTS).await;
    let actual = read(
        &mut page,
        r#"(() => {
      const target=document.getElementById('target');
      let calls=0;const fail=()=>{calls++;throw new Error('guest animation getter')};
      target.getAnimations=fail;document.getAnimations=fail;
      Object.defineProperty(Animation.prototype,'effect',{configurable:true,get:fail});
      globalThis.animationPresentation='enabled';
      return [target.getBoundingClientRect().width,getComputedStyle(target).transform,calls];
    })()"#,
    )
    .await;
    assert_eq!(actual, json!([40, "none", 0]));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_rust_session_option_overrides_its_enabled_profile() {
    let mut page = PageSession::open_with_document_response_and_options(
        EFFECTS,
        "http://127.0.0.1/",
        "",
        "",
        5000,
        "",
        ClientHints::default(),
        &[],
        BrowserStorage::default(),
        turbo_surf_render::DocumentResponseMetadata::default(),
        None,
        turbo_surf_render::PageSessionOptions {
            animation_policy: Some(turbo_surf_render::AnimationPolicy::Disabled),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        read(
            &mut page,
            "document.getElementById('target').getBoundingClientRect().width"
        )
        .await,
        json!(40)
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn profile_changes_only_affect_future_sessions_not_later_children() {
    let profile = BrowserStorage::default();
    profile
        .set_animation_policy(turbo_surf_render::AnimationPolicy::Disabled)
        .unwrap();
    let mut page = open_storage(profile.clone(), "<!doctype html><body>").await;
    profile
        .set_animation_policy(turbo_surf_render::AnimationPolicy::Enabled)
        .unwrap();
    let actual = read(&mut page, r#"(async()=>{
      const frame=document.createElement('iframe');
      frame.srcdoc='<style>@keyframes spin{to{transform:rotate(90deg)}}#target{width:40px;height:20px;animation:spin 1s infinite}</style><div id=target></div>';
      await new Promise(resolve=>{frame.onload=resolve;document.body.append(frame)});
      const target=frame.contentDocument.getElementById('target');
      return [target.getBoundingClientRect().width,frame.contentWindow.getComputedStyle(target).transform];
    })()"#).await;
    assert_eq!(actual, json!([40, "none"]));
    let mut next = open_storage(profile, EFFECTS).await;
    assert!(next
        .eval_engine_json_expression(
            "document.getElementById('target').getBoundingClientRect().width"
        )
        .await
        .is_err());
    next.close();
    page.close();
}
