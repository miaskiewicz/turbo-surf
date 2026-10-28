//! Classic `<script src="data:...">` (turbo-surf#67): parser-inserted, base64,
//! default-MIME and dynamically inserted data: scripts run in document order, a
//! child frame's data: script runs, and a data: script's error is not muted (a
//! data: response is basic-tainted). Expected log: Chrome 156 (156.0.8078.47,
//! headless) running the same page on a localhost origin.
use turbo_surf_render::PageSession;

const PAGE: &str = r#"<!doctype html><body><script>
globalThis.log=[];onerror=(m)=>{log.push('onerror '+m)};
</script>
<script src="data:text/javascript,log.push('parser')"></script>
<script src="data:text/javascript;base64,bG9nLnB1c2goJ2I2NCcp"></script>
<script src="data:text/javascript,throw new Error('boom')"></script>
<script src="data:,log.push('plain')"></script>
<script>
const s=document.createElement('script');s.src='data:text/javascript,log.push("dynamic")';s.onload=()=>log.push('dynamic load');document.body.append(s);
const c=document.createElement('iframe');c.srcdoc='<body>x</body>';document.body.append(c);
c.onload=()=>{const s=c.contentDocument.createElement('script');s.src='data:text/javascript,parent.log.push("ext")';c.contentDocument.body.append(s);};
</script></body>"#;

const CHROME_156: &str =
    r#"["parser","b64","onerror Uncaught Error: boom","plain","dynamic","dynamic load","ext"]"#;

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#67: fails until the fix"]
async fn classic_data_url_scripts_run_like_chrome() {
    let mut page = PageSession::open(PAGE, "https://frames.test/", "", "", 5000)
        .await
        .unwrap();
    let log = page.eval_json_expression("log").await.unwrap();
    assert_eq!(log, CHROME_156);
}
