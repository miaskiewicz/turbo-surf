use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn console_queue_eviction_releases_only_undelivered_argument_handles() {
    let mut page = PageSession::open("<body>ready</body>", "http://fixture.test/", "", "", 10_000)
        .await
        .unwrap();
    page.eval_expression("for(let index=0;index<1002;index++)console.log({index})")
        .await
        .unwrap();
    assert_eq!(
        page.remote_handle_diagnostics_for_testing()["retained"],
        1000
    );
    let rows: serde_json::Value = serde_json::from_str(&page.drain_network_log()).unwrap();
    let entries = rows.as_array().unwrap();
    assert_eq!(entries.len(), 1000);
    let first = entries[0]["args"][0]["id"].as_u64().unwrap();
    assert_eq!(
        page.eval_engine_expression(&format!("__tsNs.remoteHandleGet({first}).index"))
            .await
            .unwrap(),
        "2"
    );
    assert_eq!(
        page.remote_handle_diagnostics_for_testing()["retained"],
        1000
    );
    let ids: Vec<u64> = entries
        .iter()
        .map(|entry| entry["args"][0]["id"].as_u64().unwrap())
        .collect();
    page.eval_engine(&format!(
        "{}.forEach(id=>__tsNs.remoteHandleDispose(id))",
        serde_json::to_string(&ids).unwrap()
    ))
    .await
    .unwrap();
    assert_eq!(page.remote_handle_diagnostics_for_testing()["retained"], 0);
    assert_eq!(page.drain_network_log(), "[]");
}
