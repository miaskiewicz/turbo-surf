use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn host_disposal_releases_only_the_console_arguments_it_discarded() {
    let mut page = PageSession::open("<body>ready</body>", "http://fixture.test/", "", "", 10_000)
        .await
        .unwrap();
    page.eval_expression("console.log({value:'first'},{value:'second'})")
        .await
        .unwrap();
    let rows: serde_json::Value = serde_json::from_str(&page.drain_network_log()).unwrap();
    let first = u32::try_from(rows[0]["args"][0]["id"].as_u64().unwrap()).unwrap();
    let second = u32::try_from(rows[0]["args"][1]["id"].as_u64().unwrap()).unwrap();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 2);

    let host = page.network_event_log();
    host.discard_handles(vec![first]);
    page.flush_network_disposals();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 1);
    assert_eq!(
        page.eval_engine_expression(&format!("__tsNs.remoteHandleGet({second}).value"))
            .await
            .unwrap(),
        "second"
    );
    host.discard_handles(vec![second]);
    page.flush_network_disposals();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 0);
    page.flush_network_disposals();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn scrolling_a_detached_element_does_not_create_a_scroll_box_or_scroll_events() {
    let mut page = PageSession::open("<body>ready</body>", "http://fixture.test/", "", "", 10_000)
        .await
        .unwrap();
    let result = page
        .eval_json_expression(
            r#"(async()=>{
                const detached=document.createElement('div');
                detached.style.cssText='width:20px;height:20px;overflow:scroll';
                detached.innerHTML='<div style="width:500px;height:500px"></div>';
                const events=[];
                detached.addEventListener('scroll',()=>events.push('scroll'));
                detached.addEventListener('scrollend',()=>events.push('scrollend'));
                detached.scrollTo({left:80,top:90});
                detached.scrollBy({left:20,top:30});
                await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
                return {left:detached.scrollLeft,top:detached.scrollTop,events};
            })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"left":0,"top":0,"events":[]})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn replacing_host_cookies_updates_the_existing_document_and_preserves_http_only_secrecy() {
    let mut page = PageSession::open("<body>ready</body>", "http://fixture.test/", "", "", 10_000)
        .await
        .unwrap();
    page.eval_expression("document.cookie='old=stale; Path=/'")
        .await
        .unwrap();
    page.set_cookies(
        &serde_json::json!([
            {"name":"visible","value":"fresh","domain":"fixture.test","path":"/","expires_at":null,"http_only":false,"secure":false,"same_site":"lax"},
            {"name":"session","value":"host-secret","domain":"fixture.test","path":"/","expires_at":null,"http_only":true,"secure":false,"same_site":"lax"}
        ])
        .to_string(),
    );
    assert_eq!(
        page.eval_expression("document.cookie").await.unwrap(),
        "visible=fresh"
    );
    page.eval_expression("document.cookie='session=guest-write; Path=/'")
        .await
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&page.cookies()).unwrap();
    let cookies = stored.as_array().unwrap();
    assert_eq!(cookies.len(), 2);
    assert!(cookies.iter().any(|cookie| cookie["name"] == "session"
        && cookie["value"] == "host-secret"
        && cookie["http_only"] == true));
}
