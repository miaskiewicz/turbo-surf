//! Public diagnostic and retirement contracts use real session documents.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

async fn traced_child() -> PageSession {
    let mut page = PageSession::open_with_init_script(
        "<body><p id=owner>root</p><iframe srcdoc='<body>child</body>'></iframe></body>",
        "https://snapshot.test/source",
        "",
        "",
        5000,
        "globalThis.originalStringify=JSON.stringify;globalThis[Symbol.for('turbo-surf.interface-trace')]={snapshot:()=>({marker:'real-child'})};",
    )
    .await
    .unwrap();
    page.read_engine("globalThis.snapshotChildDocument=document.querySelector('iframe').contentDocument;return true")
        .unwrap();
    page
}

fn child_snapshot(page: &mut PageSession) -> Value {
    let rows = page.frame_interface_snapshots();
    assert_eq!(rows.len(), 1, "{rows:?}");
    rows.into_iter().next().unwrap()
}

async fn recover_snapshot(page: &mut PageSession) {
    page.eval("document.querySelector('iframe').contentWindow.JSON.stringify=document.querySelector('iframe').contentWindow.originalStringify")
        .await
        .unwrap();
    assert_eq!(
        child_snapshot(page),
        json!({"url":"about:srcdoc","interfaces":{"marker":"real-child"}})
    );
    assert_eq!(
        page.read_engine("return [document.querySelector('#owner').textContent,document.querySelector('iframe').contentDocument===snapshotChildDocument]")
            .unwrap(),
        "[\"root\",true]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn frame_snapshot_reports_a_child_exception_and_recovers_without_replacement() {
    let mut page = traced_child().await;
    let root = page.auxiliary_root_id().unwrap();
    assert_eq!(
        child_snapshot(&mut page)["interfaces"]["marker"],
        "real-child"
    );
    page.eval("document.querySelector('iframe').contentWindow.JSON.stringify=()=>{throw Error('child-snapshot-failed')}")
        .await
        .unwrap();
    assert_eq!(
        child_snapshot(&mut page),
        json!({"error":"child interface snapshot failed"})
    );
    recover_snapshot(&mut page).await;
    assert_eq!(page.auxiliary_root_id().unwrap(), root);
}

#[tokio::test(flavor = "current_thread")]
async fn frame_snapshot_reports_invalid_json_without_poisoning_the_child() {
    let mut page = traced_child().await;
    page.eval("document.querySelector('iframe').contentWindow.JSON.stringify=()=>'{'")
        .await
        .unwrap();
    let row = child_snapshot(&mut page);
    let error = row["error"]
        .as_str()
        .expect("explicit JSON parse diagnostic");
    assert!(error.contains("EOF"), "{row}");
    recover_snapshot(&mut page).await;
}

#[tokio::test(flavor = "current_thread")]
async fn retired_root_does_not_seed_sticky_activation_into_a_new_document() {
    let mut source = PageSession::open(
        "<body><button id=go>activate</button><script>document.querySelector('#go').onclick=e=>globalThis.actualTrustedClick=e.isTrusted</script></body>",
        "https://activation-snapshot.test/source",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    source.input(&json!([{"click":"#go"}])).await.unwrap();
    assert_eq!(
        source
            .eval_json_expression("[actualTrustedClick,navigator.userActivation.hasBeenActive]")
            .await
            .unwrap(),
        "[true,true]"
    );
    let live = source.navigation_activation_snapshot();
    let root = source.auxiliary_root_id().unwrap();
    assert!(!source.close_target(root).unwrap());
    let retired = source.navigation_activation_snapshot();
    assert!(source.script_nav_activation_snapshot().is_none());
    assert!(source.serialize_checked().is_err());
    source.close();
    for (activation, expected) in [(live, true), (retired, false)] {
        let mut destination = PageSession::open_with_document_response_and_activation(
            "<body><script>globalThis.activationAtParse=navigator.userActivation.hasBeenActive</script></body>",
            "https://activation-snapshot.test/destination",
            "",
            "",
            5000,
            "globalThis.activationAtInit=navigator.userActivation.hasBeenActive",
            Default::default(),
            &[],
            Default::default(),
            Default::default(),
            Some(activation),
        )
        .await
        .unwrap();
        let actual: Value = serde_json::from_str(
            &destination
                .eval_json_expression("[activationAtInit,activationAtParse,navigator.userActivation.hasBeenActive,navigator.userActivation.isActive]")
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(actual, json!([expected, expected, expected, false]));
        destination.close();
    }
}
