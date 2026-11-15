use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_input_implicit_submit_uses_actual_owner_and_keeps_trusted_clicks() {
    for kind in ["text", "checkbox", "radio", "range"] {
        let html = format!(
            r#"<form><input id='q' type='{kind}'><button id='submit'>Submit</button></form><script>
        globalThis.events=[];
        document.addEventListener('click',event=>events.push(['click',event.target.id,event.isTrusted]));
        document.querySelector('form').addEventListener('submit',event=>{{event.preventDefault();events.push(['submit',event.submitter?.id??null]);}});
        </script>"#
        );
        let mut page = PageSession::open(&html, "http://example.test/", "", "", 5000)
            .await
            .unwrap();
        page.input(&serde_json::json!([{"focus":"#q"},{"press":"Enter"}]))
            .await
            .unwrap();
        assert_eq!(
            page.eval_expression("JSON.stringify(events)")
                .await
                .unwrap(),
            r#"[["click","submit",true],["submit","submit"]]"#,
            "{kind}"
        );
        page.close();
    }
}
