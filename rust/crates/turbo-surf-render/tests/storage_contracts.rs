//! Storage identity must follow real native origins, including opaque sandboxes.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn same_origin_frames_share_host_storage_and_keep_session_areas_separate() {
    let mut page = PageSession::open(
        "<body><iframe id=child srcdoc='<p>child</p>'></iframe></body>",
        "https://storage.test/page",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = page.eval(r#"globalThis.__RESULT=(()=>{
      localStorage.clear();sessionStorage.clear();localStorage.setItem('first','parent');
      const child=document.querySelector('iframe').contentWindow;
      child.localStorage.setItem('second','child');child.sessionStorage.setItem('first','session');
      const borrowed=Storage.prototype.getItem.call(child.localStorage,'second');
      const descriptor=Object.getOwnPropertyDescriptor(Storage.prototype,'length');
      let brand=false,symbol=false;try{descriptor.get.call({})}catch(e){brand=e instanceof TypeError}
      try{localStorage.setItem(Symbol(),'bad')}catch(e){symbol=e instanceof TypeError}
      return JSON.stringify({same:child.localStorage.getItem('first'),parent:localStorage.second,
        session:sessionStorage.first,separate:localStorage.first,keys:Object.keys(localStorage),
        stable:localStorage===localStorage,brand,symbol,borrowed});
    })()"#).await.unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        result,
        serde_json::json!({"same":"parent","parent":"child","session":"session","separate":"parent","keys":["first","second"],"stable":true,"brand":true,"symbol":true,"borrowed":"child"})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pending_navigation_does_not_grant_a_different_origin_storage_area() {
    let mut page = PageSession::open("<body></body>", "https://first.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval("globalThis.retained=localStorage;retained.setItem('key','first');location.href='https://second.test/';").await.unwrap();
    assert_eq!(page.eval("globalThis.__RESULT=JSON.stringify([localStorage.getItem('key'),retained.getItem('key'),retained===localStorage])").await.unwrap(), "[\"first\",\"first\",true]");
}

#[tokio::test(flavor = "current_thread")]
async fn root_opaque_origins_cannot_acquire_a_storage_capability() {
    let mut page = PageSession::open("<body></body>", "about:blank", "", "", 5000)
        .await
        .unwrap();
    let result=page.eval(r#"globalThis.__RESULT=JSON.stringify(['localStorage','sessionStorage'].map(name=>{try{window[name];return null}catch(e){return [e.name,e instanceof DOMException]}}))"#).await.unwrap();
    assert_eq!(
        result,
        "[[\"SecurityError\",true],[\"SecurityError\",true]]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn browsing_storage_survives_navigation_and_independent_handles_stay_isolated() {
    let backing = turbo_surf_render::BrowserStorage::default();
    let mut first = PageSession::open_with_browsing_state(
        "<body></body>",
        "https://persistent.test/first",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
        backing.clone(),
    )
    .await
    .unwrap();
    first
        .eval("localStorage.setItem('local','value');sessionStorage.setItem('session','value');")
        .await
        .unwrap();
    first.close();
    let mut second = PageSession::open_with_browsing_state(
        "<body></body>",
        "https://persistent.test/second",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
        backing.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        second
            .eval("globalThis.__RESULT=JSON.stringify([localStorage.local,sessionStorage.session])")
            .await
            .unwrap(),
        "[\"value\",\"value\"]"
    );
    second.close();
    let mut other_origin = PageSession::open_with_browsing_state(
        "<body></body>",
        "https://different.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[],
        backing,
    )
    .await
    .unwrap();
    assert_eq!(
        other_origin
            .eval("globalThis.__RESULT=JSON.stringify([localStorage.length,sessionStorage.length])")
            .await
            .unwrap(),
        "[0,0]"
    );
    other_origin.close();
    let mut independent =
        PageSession::open("<body></body>", "https://persistent.test/", "", "", 5000)
            .await
            .unwrap();
    assert_eq!(
        independent
            .eval("globalThis.__RESULT=JSON.stringify([localStorage.length,sessionStorage.length])")
            .await
            .unwrap(),
        "[0,0]"
    );
}
