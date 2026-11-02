//! Web Storage quota edges: an oversized key reads as absent without materializing,
//! writes beyond the 5 MiB area quota throw QuotaExceededError (replacing a value
//! counts only the other entries), and a foreign receiver is an illegal invocation.
//! Expected: Chrome 156 (156.0.8078.4, headless) running the same script on localhost.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"(()=>{const log=[];const t=(n,f)=>{try{log.push([n,f()])}catch(e){log.push([n,e.name+': '+(e instanceof DOMException)])}};
localStorage.clear();sessionStorage.clear();
const big='x'.repeat(5*1024*1024+1);
t('get huge',()=>localStorage.getItem(big));
t('remove huge',()=>localStorage.removeItem(big)===undefined);
t('set over quota',()=>localStorage.setItem('k',big));
t('set half',()=>{localStorage.setItem('a','y'.repeat(2.5*1024*1024));return localStorage.length});
t('set second half',()=>localStorage.setItem('b','z'.repeat(2.6*1024*1024)));
t('replace same size',()=>{localStorage.setItem('a','w'.repeat(2.5*1024*1024));return localStorage.getItem('a').length});
t('length',()=>localStorage.length);
t('illegal get',()=>Storage.prototype.getItem.call({},'a'));
t('cross area',()=>Storage.prototype.getItem.call(sessionStorage,'a'));
t('key()',()=>localStorage.key(0));
localStorage.clear();
t('clear empty',()=>{localStorage.clear();return localStorage.length});
return log;})()"#;

const CHROME_156: &str = r#"[["get huge",null],["remove huge",true],["set over quota","QuotaExceededError: true"],["set half",1],["set second half","QuotaExceededError: true"],["replace same size",2621440],["length",1],["illegal get","TypeError: false"],["cross area",null],["key()","a"],["clear empty",0]]"#;

#[tokio::test(flavor = "current_thread")]
async fn storage_quota_and_receiver_edges_match_chrome() {
    let mut page = PageSession::open("<body>", "https://quota.test/", "", "", 20000)
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression(SCRIPT).await.unwrap(), CHROME_156);
}
