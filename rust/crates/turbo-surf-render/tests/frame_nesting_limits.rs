//! Frame creation limits. An iframe inserted into a removed frame's (inactive)
//! document gets no browsing context and no exception, like Chrome 156. Nesting is
//! capped natively at 12 frame levels below the page (Chrome allows deeper nesting):
//! the 13th level gets no window.
use turbo_surf_render::PageSession;

const NO_BUDGET_MS: u64 = 600_000;

#[tokio::test(flavor = "current_thread")]
async fn inactive_parents_and_the_native_depth_cap_create_no_window() {
    // The session budget is wall-clock time, and it is not what this test checks.
    // Each nested frame bootstraps a full window realm: about 0.7 to 2.2 s apiece in a
    // debug build at load average 50 to 90 (round 3 measurement), so 13 levels overran
    // a 10 s budget on a busy machine. The bound below cannot be reached by the
    // 14 frame creations, so the result depends only on the nesting rules.
    let mut page = PageSession::open("<body>", "https://nesting.test/", "", "", NO_BUDGET_MS)
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
