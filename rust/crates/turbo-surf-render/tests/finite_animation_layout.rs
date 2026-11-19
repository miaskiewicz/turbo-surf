use serde_json::Value;
use turbo_surf_render::PageSession;

const PAGE: &str = "<!doctype html><style>\
@keyframes fadeInUp{from{opacity:0;transform:translateY(8px)}to{opacity:1;transform:translateY(0)}}\
#message{animation:fadeInUp 0.3s ease-out}button{width:32px;height:32px;padding:0;border:0}\
</style><div id=message><button id=feedback>Vote</button></div>";

async fn observe(expression: &str) -> Value {
    let mut page = PageSession::open(PAGE, "http://127.0.0.1/", "", "", 5000)
        .await
        .unwrap();
    let raw = page.eval_engine_json_expression(expression).await.unwrap();
    page.close();
    serde_json::from_str(&raw).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn completed_non_filling_entrance_animation_releases_feedback_geometry() {
    let actual = observe(
        r#"(async()=>{
          const message=document.getElementById('message');
          const animations=message.getAnimations();
          await Promise.all(animations.map(animation=>animation.finished));
          let jobs=0;queueMicrotask(()=>jobs++);
          const rect=document.getElementById('feedback').getBoundingClientRect();
          return [message.getAnimations().length,rect.width,rect.height,jobs];
        })()"#,
    )
    .await;
    assert_eq!(actual, serde_json::json!([0, 32, 32, 0]));
}

#[tokio::test(flavor = "current_thread")]
async fn contributing_entrance_effects_remain_strict() {
    let actual = observe(
        r#"(()=>{
          const message=document.getElementById('message');
          const feedback=document.getElementById('feedback');
          const results=[];
          for(const declaration of [
            'fadeInUp 100s linear',
            'fadeInUp 1s linear infinite',
            'fadeInUp 100s linear paused',
            'fadeInUp 1ms linear forwards'
          ]){
            message.style.animation=declaration;
            const animations=message.getAnimations();
            if(declaration.includes('forwards'))for(const animation of animations)animation.finish();
            try{feedback.getBoundingClientRect();results.push('admitted');}
            catch(error){results.push(error.message.includes('animation resource'));}
          }
          return results;
        })()"#,
    )
    .await;
    assert_eq!(actual, serde_json::json!([true, true, true, true]));
}

#[tokio::test(flavor = "current_thread")]
async fn guest_animation_methods_cannot_forge_inactive_geometry() {
    let actual = observe(
        r#"(()=>{
          const message=document.getElementById('message');
          message.style.animation='fadeInUp 100s linear';
          message.getAnimations();
          Document.prototype.getAnimations=()=>[];
          Element.prototype.getAnimations=()=>[];
          Object.defineProperty(CSSAnimation.prototype,'animationName',{get:()=>''});
          Object.defineProperty(Animation.prototype,'effect',{get:()=>null});
          try{document.getElementById('feedback').getBoundingClientRect();return 'admitted';}
          catch(error){return error.message.includes('animation resource');}
        })()"#,
    )
    .await;
    assert_eq!(actual, serde_json::json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn playback_changes_without_dom_mutation_cannot_reuse_completed_geometry() {
    let actual = observe(
        r#"(async()=>{
          const message=document.getElementById('message');
          const [animation]=message.getAnimations();
          await animation.finished;
          const feedback=document.getElementById('feedback');
          const before=feedback.getBoundingClientRect().width;
          const results=[];
          for(const change of [()=>animation.play(),()=>animation.currentTime=0,()=>animation.finish()]){
            change();
            try{feedback.getBoundingClientRect();results.push('admitted');}
            catch(error){results.push(error.message.includes('animation resource'));}
          }
          return [before,results];
        })()"#,
    ).await;
    assert_eq!(actual, serde_json::json!([32, [true, true, true]]));
}

#[tokio::test(flavor = "current_thread")]
async fn declaration_restarts_and_other_ancestors_keep_their_guards() {
    let actual = observe(
        r#"(async()=>{
          const message=document.getElementById('message');
          await Promise.all(message.getAnimations().map(animation=>animation.finished));
          const feedback=document.getElementById('feedback');
          const before=feedback.getBoundingClientRect().width;
          document.body.style.animation='fadeInUp 100s linear';
          let ancestor;
          try{feedback.getBoundingClientRect();ancestor='admitted';}
          catch(error){ancestor=error.message.includes('animation resource');}
          document.body.style.animation='none';
          message.style.animation='fadeInUp 100s linear';
          let restart;
          try{feedback.getBoundingClientRect();restart='admitted';}
          catch(error){restart=error.message.includes('animation resource');}
          return [before,ancestor,restart];
        })()"#,
    )
    .await;
    assert_eq!(actual, serde_json::json!([32, true, true]));
}

#[tokio::test(flavor = "current_thread")]
async fn active_shadow_stylesheet_effects_keep_their_geometry_guard() {
    let actual = observe(
        r#"(async()=>{
          document.getElementById('message').style.animation='none';
          const results=[];
          for(const mode of ['open','closed']){
            const host=document.createElement('section');document.body.append(host);
            const root=host.attachShadow({mode});
            root.innerHTML='<style>@keyframes local{from{transform:translateY(8px)}to{transform:translateY(0)}}div{animation:local 100s!important}button{width:32px;height:32px;padding:0;border:0}</style><div><button>Vote</button></div>';
            const message=root.querySelector('div');
            const animations=message.getAnimations();
            await Promise.all(animations.map(animation=>animation.ready));
            const target=root.querySelector('button');
            let guarded=false;
            try{target.getBoundingClientRect();}
            catch(error){guarded=error.message.includes('animation resource');}
            results.push([animations.length,animations[0]?.playState,guarded]);
          }
          return results;
        })()"#,
    ).await;
    // Scoped effects are now canonically discoverable. Active effects must still
    // reject geometry; waiting for their 100-second completion would test another phase.
    assert_eq!(
        actual,
        serde_json::json!([[1, "running", true], [1, "running", true]])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn completed_shadow_effects_admit_geometry_and_restart_revokes_the_proof() {
    let actual = observe(
        r#"(async()=>{
          document.getElementById('message').style.animation='none';
          const results=[];
          for(const mode of ['open','closed']){
            const host=document.createElement('section');document.body.append(host);
            const root=host.attachShadow({mode});
            root.innerHTML='<style>@keyframes local{from{transform:translateY(8px)}to{transform:translateY(0)}}div{animation:local 64ms}button{width:32px;height:32px;padding:0;border:0}</style><div><button>Vote</button></div>';
            const message=root.querySelector('div'),target=root.querySelector('button');
            const animations=message.getAnimations();
            await Promise.all(animations.map(animation=>animation.finished));
            let jobs=0;queueMicrotask(()=>jobs++);
            const rect=target.getBoundingClientRect();
            const checkpointJobs=jobs;
            message.style.animation='local 100s';
            let restartGuarded=false;
            try{target.getBoundingClientRect();}
            catch(error){restartGuarded=error.message.includes('animation resource');}
            results.push([animations.length,rect.width,rect.height,checkpointJobs,restartGuarded]);
          }
          return results;
        })()"#,
    ).await;
    assert_eq!(
        actual,
        serde_json::json!([[1, 32, 32, 0, true], [1, 32, 32, 0, true]])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_geometry_never_calls_tampered_animation_getters() {
    let actual = observe(
        r#"(async()=>{
          const message=document.getElementById('message');
          await Promise.all(message.getAnimations().map(animation=>animation.finished));
          globalThis.getterCalls=0;
          const fail=()=>{getterCalls++;throw Error('guest animation hook')};
          Document.prototype.getAnimations=fail;
          Element.prototype.getAnimations=fail;
          Object.defineProperty(CSSAnimation.prototype,'animationName',{get:fail});
          Object.defineProperty(Animation.prototype,'effect',{get:fail});
          KeyframeEffect.prototype.getComputedTiming=fail;
          let result;
          try{result=document.getElementById('feedback').getBoundingClientRect().width;}
          catch(error){result=error.message.includes('animation resource');}
          return [result,getterCalls];
        })()"#,
    )
    .await;
    // Producer-owned records can prove completion without calling these Web APIs.
    assert_eq!(actual, serde_json::json!([32, 0]));
}

#[tokio::test(flavor = "current_thread")]
async fn completed_child_document_effects_do_not_borrow_parent_permissions() {
    let actual = observe(
        r#"(async()=>{
          const parent=document.getElementById('message');
          parent.style.animation='none';
          const frame=document.createElement('iframe');
          await new Promise(resolve=>{
            frame.onload=resolve;
            frame.srcdoc='<style>@keyframes local{from{transform:translateY(8px)}to{transform:translateY(0)}}div{animation:local .3s}button{width:32px;height:32px;padding:0;border:0}</style><div><button>Vote</button></div>';
            document.body.append(frame);
          });
          const child=frame.contentDocument;
          const message=child.querySelector('div');
          await Promise.all(message.getAnimations().map(animation=>animation.finished));
          const feedback=child.querySelector('button');
          const before=feedback.getBoundingClientRect().width;
          message.style.animation='local 100s linear';
          let active;
          try{feedback.getBoundingClientRect();active='admitted';}
          catch(error){active=error.message.includes('animation resource');}
          return [before,active];
        })()"#,
    ).await;
    assert_eq!(actual, serde_json::json!([32, true]));
}
