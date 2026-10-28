//! A sandboxed same-origin iframe sets `top.location.href` (turbo-surf#84). The page's
//! own Location setters must apply HTML's "allowed by sandboxing to navigate" check in
//! both engine configurations. Expected values: the authored Chrome 155 control
//! (harness/detection/sandbox-navigation-chrome155.json, cases `unrestricted`,
//! `sandbox-blocked`, `sandbox-top-allowed`, `sandbox-activation-required`,
//! `nested-inner-block`), which Chrome 156 matches.
use turbo_surf_render::PageSession;

fn frame(sandbox: &str, body: &str) -> String {
    let attribute = if sandbox.is_empty() {
        String::new()
    } else {
        format!(" sandbox=\"{sandbox}\"")
    };
    format!("<iframe{attribute} srcdoc=\"{body}\"></iframe>")
}

// The innermost document: navigates top to a fragment on request and reports the error.
const CHILD: &str =
    "<script>addEventListener('message',function(event){if(event.data!=='run')return;\
let error=null;try{top.location.href='#checked';}catch(value){error=value.name+':'+value.message;}\
top.postMessage({kind:'result',error},'*');});</script>";

const PROBE: &str = "(async()=>{let target=frames[0];while(target.length)target=target[0];\
const error=await new Promise(resolve=>{addEventListener('message',e=>{if(e.data&&e.data.kind==='result')resolve(e.data.error)});\
target.postMessage('run','*');});await new Promise(r=>setTimeout(r,50));\
return {error,changed:location.hash==='#checked'};})()";

async fn run(html: String) -> serde_json::Value {
    let mut page = PageSession::open(&html, "http://localhost/control", "", "", 10000)
        .await
        .unwrap();
    let raw = page.eval_json_expression(PROBE).await.unwrap();
    page.close();
    serde_json::from_str(&raw).unwrap()
}

const DENIED: &str = "SecurityError:The current window does not have permission to navigate the target frame to 'http://localhost/control#checked'.";

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#84: fails until the fix"]
async fn same_origin_sandboxed_frames_navigate_top_only_with_allow_top_navigation() {
    let child = CHILD.replace('\'', "&#39;").replace('<', "&lt;");
    for (id, html, error) in [
        ("unrestricted", frame("", &child), None),
        (
            "sandbox-blocked",
            frame("allow-scripts allow-same-origin", &child),
            Some(DENIED),
        ),
        (
            "sandbox-top-allowed",
            frame(
                "allow-scripts allow-same-origin allow-top-navigation",
                &child,
            ),
            None,
        ),
        (
            "sandbox-activation-required",
            frame(
                "allow-scripts allow-same-origin allow-top-navigation-by-user-activation",
                &child,
            ),
            Some(DENIED),
        ),
        (
            "nested-inner-block",
            frame(
                "allow-scripts allow-same-origin allow-top-navigation",
                &frame("allow-scripts allow-same-origin", &child)
                    .replace('&', "&amp;")
                    .replace('"', "&quot;"),
            ),
            Some(DENIED),
        ),
    ] {
        assert_eq!(
            run(format!("<!doctype html><body>{html}</body>")).await,
            serde_json::json!({"error": error, "changed": error.is_none()}),
            "{id}"
        );
    }
}
