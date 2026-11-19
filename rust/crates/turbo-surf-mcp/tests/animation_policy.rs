//! The automation policy is selected on the session, before ephemeral render runtimes.
use serde_json::json;
use turbo_surf_mcp::{call_tool, Session};

#[tokio::test(flavor = "current_thread")]
async fn set_mode_reports_and_validates_animation_presentation_policy() {
    let mut session = Session::new();
    let selected = call_tool(
        &mut session,
        "set_mode",
        &json!({"mode":"fast","animations":"disabled"}),
    )
    .await
    .unwrap();
    assert_eq!(selected["animations"], json!("disabled"));
    assert!(call_tool(
        &mut session,
        "set_mode",
        &json!({"mode":"fast","animations":"fast-forward"})
    )
    .await
    .is_err());
    let retained = call_tool(&mut session, "set_mode", &json!({"mode":"fast"}))
        .await
        .unwrap();
    assert_eq!(retained, json!({"mode":"fast"}));
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_render_installs_disabled_presentation_before_native_geometry() {
    let mut session = Session::new();
    session.load("http://127.0.0.1/", "<!doctype html><style>@keyframes spin{to{transform:rotate(90deg)}}#target{width:40px;height:20px;transform:none;animation:spin 1s infinite}</style><div id=target></div>");
    call_tool(
        &mut session,
        "set_mode",
        &json!({"mode":"fast","animations":"disabled"}),
    )
    .await
    .unwrap();
    call_tool(&mut session, "set_mode", &json!({"mode":"fast"}))
        .await
        .unwrap();
    call_tool(&mut session,"render",&json!({"script":"const target=document.getElementById('target');target.textContent=JSON.stringify([target.getBoundingClientRect().width,getComputedStyle(target).transform,typeof target.animate]);"})).await.unwrap();
    assert_eq!(
        call_tool(&mut session, "text_content", &json!({"selector":"#target"}))
            .await
            .unwrap(),
        json!(r#"[40,"none","function"]"#)
    );
    call_tool(
        &mut session,
        "set_mode",
        &json!({"mode":"fast","animations":"enabled"}),
    )
    .await
    .unwrap();
    let error = call_tool(
        &mut session,
        "render",
        &json!({"script":"document.getElementById('target').getBoundingClientRect().width"}),
    )
    .await
    .unwrap_err();
    assert!(error.contains("animation resource"), "{error}");
}
