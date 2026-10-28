//! Storage Access API on a top-level document and its same-origin child: access is
//! already granted, the unpartitioned request resolves, and handle requests are
//! validated like Chrome. Expected: Chrome 156 (156.0.8078.4, headless) running the
//! same script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"(async()=>{const log=[];const t=async(n,f)=>{try{log.push([n,await f()])}catch(e){log.push([n,e.name])}};
await t('has',()=>document.hasStorageAccess());
await t('hasUnpartitioned',()=>document.hasUnpartitionedCookieAccess?document.hasUnpartitionedCookieAccess():'n/a');
await t('request',()=>document.requestStorageAccess().then(v=>String(v)));
await t('handle',async()=>{const h=await document.requestStorageAccess({localStorage:true,sessionStorage:true});h.localStorage.setItem('k','v');return [h.constructor.name,h.localStorage.getItem('k'),h.localStorage===localStorage,typeof h.sessionStorage.length]});
await t('handle empty',()=>document.requestStorageAccess({}).then(h=>h.constructor.name));
await t('handle missing',async()=>{const h=await document.requestStorageAccess({cookies:true});try{h.localStorage}catch(e){return e.name}return 'no throw'});
const f=document.createElement('iframe');document.body.append(f);
await t('frame has',()=>f.contentDocument.hasStorageAccess());
return log;})()"#;

const CHROME_156: &str = r#"[["has",true],["hasUnpartitioned",true],["request","undefined"],["handle","TypeError"],["handle empty","SecurityError"],["handle missing","TypeError"],["frame has",true]]"#;

#[tokio::test(flavor = "current_thread")]
async fn storage_access_requests_match_chrome() {
    let mut page = PageSession::open("<body>", "https://access.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression(SCRIPT).await.unwrap(), CHROME_156);
}

/// turbo-surf#94: Chrome 156 (156.0.8078.47, headless, localhost page) does not expose
/// `StorageAccessHandle` as a window global.
#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#94: fails until the fix"]
async fn storage_access_handle_is_not_a_window_global() {
    let mut page = PageSession::open("<body>", "https://access.test/", "", "", 5000)
        .await
        .unwrap();
    let got = page
        .eval_json_expression(
            "[typeof StorageAccessHandle,'StorageAccessHandle' in window,Object.getOwnPropertyNames(window).includes('StorageAccessHandle')]",
        )
        .await
        .unwrap();
    assert_eq!(got, r#"["undefined",false,false]"#);
}
