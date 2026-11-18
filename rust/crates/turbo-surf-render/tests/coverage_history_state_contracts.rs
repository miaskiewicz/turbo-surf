use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn denied_history_url_rewrites_are_atomic_and_leave_valid_recovery_available() {
    let mut page = PageSession::open(
        "<title>history</title>",
        "https://history.test/start",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let actual = page.eval_target_expression(root, r#"(()=>{
      history.replaceState({marker:'original'},'', '/start');
      const errors=[];
      for(const url of ['http://history.test/other','https://user@history.test/other','https://history.test:444/other','https://other.test/other']){
        try{history.pushState({marker:'bad'},'',url);errors.push('accepted')}
        catch(error){errors.push(error.name)}
      }
      const unchanged=[history.length,history.state.marker,location.href];
      history.pushState({marker:'valid'},'', '/recovered?q=1#ok');
      return [errors,unchanged,history.length,history.state.marker,location.pathname,location.search,location.hash];
    })()"#, false, true).await.unwrap();
    assert_eq!(
        actual,
        r##"[["SecurityError","SecurityError","SecurityError","SecurityError"],[1,"original","https://history.test/start"],2,"valid","/recovered","?q=1","#ok"]"##
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn file_history_rewrites_preserve_document_path_but_allow_query_and_fragment() {
    let mut page = PageSession::open(
        "<title>local</title>",
        "file:///tmp/history.html",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let actual = page
        .eval_target_expression(
            root,
            r#"(()=>{
      history.replaceState({n:1},'', 'file:///tmp/history.html?q=1#one');
      let denied='accepted';
      try{history.pushState({n:2},'', 'file:///tmp/other.html')}catch(error){denied=error.name}
      return [denied,history.length,history.state.n,location.href];
    })()"#,
            false,
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        actual,
        r#"["SecurityError",1,1,"file:///tmp/history.html?q=1#one"]"#
    );
    page.close();
}
