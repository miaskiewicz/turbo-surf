//! Scripts a parent inserts into child documents: a live frame runs them, a removed
//! (retired) frame or one sandboxed without allow-scripts never does, and a srcdoc
//! frame runs inserted inline scripts (`data:` script URLs: turbo-surf#67). Expected log: Chrome 156 (156.0.8078.4,
//! headless) running the same script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"globalThis.log=[];
const run=(doc,label)=>{const s=doc.createElement('script');s.textContent="parent.log.push('"+label+"')";try{doc.body.append(s);log.push(label+' appended');}catch(e){log.push(label+' '+e.name)}};
const a=document.createElement('iframe');document.body.append(a);const ad=a.contentDocument;ad.body||ad.documentElement.append(ad.createElement('body'));
run(ad,'live');
a.remove();run(ad,'retired');
const b=document.createElement('iframe');b.sandbox='allow-same-origin';document.body.append(b);run(b.contentDocument,'sandboxed');
const c=document.createElement('iframe');c.srcdoc='<body>x</body>';document.body.append(c);
c.onload=()=>{run(c.contentDocument,'srcdoc');};"#;

const CHROME_156: &str = r#"["live","live appended","retired appended","sandboxed appended","srcdoc","srcdoc appended"]"#;

#[tokio::test(flavor = "current_thread")]
async fn retired_and_sandboxed_frames_never_run_inserted_scripts() {
    let html = format!("<!doctype html><body><script>{SCRIPT}</script></body>");
    let mut page = PageSession::open(&html, "https://frames.test/", "", "", 5000)
        .await
        .unwrap();
    let log = page.eval_json_expression("log").await.unwrap();
    assert_eq!(log, CHROME_156);
}

/// Chrome 156 runs the classic script, then the deferred module: `["classic","module"]`.
/// The stock SDK has no per-frame module loader, so only the native engine runs the
/// module; the stock build reports that limit and still runs the classic script.
#[tokio::test(flavor = "current_thread")]
async fn child_frame_module_scripts_need_the_native_engine() {
    let html = r#"<!doctype html><body><script>
globalThis.ran=[];const f=document.createElement('iframe');
f.srcdoc='<script type=module>parent.ran.push("module")<\/script><script>parent.ran.push("classic")<\/script>';
document.body.append(f);
</script></body>"#;
    let mut page = PageSession::open(html, "https://frames.test/", "", "", 5000)
        .await
        .unwrap();
    let expected = if cfg!(turbo_surf_native_proxy) {
        r#"["classic","module"]"#
    } else {
        r#"["classic"]"#
    };
    assert_eq!(page.eval_json_expression("ran").await.unwrap(), expected);
}
