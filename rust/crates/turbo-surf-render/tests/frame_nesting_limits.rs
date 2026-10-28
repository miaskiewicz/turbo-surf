//! Frame creation limits. An iframe inserted into a removed frame's (inactive)
//! document gets no browsing context and no exception, like Chrome 156. Nesting is
//! capped natively at 12 frame levels below the page (Chrome allows deeper nesting):
//! the 13th level gets no window.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn inactive_parents_and_the_native_depth_cap_create_no_window() {
    let mut page = PageSession::open("<body>", "https://nesting.test/", "", "", 10000)
        .await
        .unwrap();
    let value = page
        .eval_json_expression(
            r#"(()=>{const log=[];let doc=document;let depth=0;
for(let i=0;i<14;i++){const f=doc.createElement('iframe');doc.body.append(f);if(!f.contentWindow){break}depth++;doc=f.contentDocument;}
log.push(depth);
const a=document.createElement('iframe');document.body.append(a);const ad=a.contentDocument;a.remove();
const c=ad.createElement('iframe');let err='none';try{ad.body.append(c)}catch(e){err=e.name}
log.push(err,c.contentWindow,c.contentDocument);
return log;})()"#,
        )
        .await
        .unwrap();
    assert_eq!(value, r#"[12,"none",null,null]"#);
}
