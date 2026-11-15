#[path = "support/popup_contract.rs"]
pub mod fixture;
use turbo_surf_render::{AuxiliaryHostServices, NativeHistoryLog};

#[tokio::test(flavor = "current_thread")]
async fn interrupted_history_mutations_retire_only_the_actual_changed_document() {
    for (name, expression, driver_back, read) in [
        (
            "push",
            "history.pushState({changed:true},'', '/changed')",
            false,
            false,
        ),
        (
            "replace",
            "history.replaceState({changed:true},'', '/changed')",
            false,
            false,
        ),
        ("selection", "", true, false),
        ("scroll", "history.scrollRestoration='manual'", false, false),
        (
            "navigation state",
            "navigation.updateCurrentEntry({state:{changed:true}})",
            false,
            false,
        ),
        ("input callback", "", false, false),
        ("set content", "", false, false),
        (
            "read callback",
            "return globalThis.readMutation.value",
            false,
            true,
        ),
    ] {
        let log = NativeHistoryLog::default();
        let mut page = fixture::open(
            "https://history.test",
            AuxiliaryHostServices {
                history_log: log.clone(),
                ..Default::default()
            },
        )
        .await;
        let root = page.auxiliary_root_id().unwrap();
        page.eval_target_expression(root, "globalThis.ownerMark={};history.pushState({n:1},'', '/one');history.pushState({n:2},'', '/two')", false, false).await.unwrap();
        let peer = fixture::child(&mut page).await;
        page.eval_target_expression(peer, "globalThis.peerMark={};globalThis.savedPeerMark=peerMark;history.replaceState({peer:true},'', 'about:blank')", false, false).await.unwrap();
        if name == "input callback" {
            page.eval_target_expression(root, "const q=document.createElement('input');q.id='history-input';document.body.append(q);q.addEventListener('input',()=>history.pushState({changed:true},'', '/changed'))", false, false).await.unwrap();
        }
        if name == "read callback" {
            page.eval_target_expression(root, "globalThis.readMutation={get value(){history.replaceState({changed:true},'', '/changed');return 1}}", false, false).await.unwrap();
        }
        page.interrupt_next_history_commit_for_testing(root)
            .unwrap();
        let result = if name == "input callback" {
            page.input(&serde_json::json!([{"focus":"#history-input"},{"type":"x"}]))
                .await
                .map(|_| String::new())
        } else if name == "set content" {
            page.set_content(
                "<body><script>history.pushState({changed:true},'','/changed')</script></body>",
                "load",
                None,
            )
            .await
            .map(|_| String::new())
        } else if driver_back {
            page.navigate_target(root, "", "back").await
        } else if read {
            page.read_target_engine(root, expression)
        } else {
            page.eval_target_expression(root, expression, false, false)
                .await
        };
        assert!(
            result.is_err(),
            "{name}: mutation returned success after interruption"
        );
        let info = page.auxiliary_event_log().info(&root.to_string()).unwrap();
        assert!(
            info.closed,
            "{name}: unacknowledged changed document remained active"
        );
        assert!(
            !page
                .auxiliary_event_log()
                .info(&peer.to_string())
                .unwrap()
                .closed,
            "{name}: unrelated peer closed"
        );
        assert_eq!(
            page.eval_target_expression(
                peer,
                "peerMark===savedPeerMark&&history.state.peer===true",
                false,
                true
            )
            .await
            .unwrap(),
            "true",
            "{name}: peer realm/state was replaced"
        );
        assert!(log
            .snapshot(&root.to_string())
            .unwrap_err()
            .contains("closed"));
        assert!(log.snapshot(&peer.to_string()).is_ok());
        page.close();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn ordinary_execution_timeout_without_history_mutation_does_not_retire_the_target() {
    let mut page = fixture::open("https://history.test", Default::default()).await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval_target_expression(
        root,
        "globalThis.ownerMark={};globalThis.savedOwnerMark=ownerMark",
        false,
        false,
    )
    .await
    .unwrap();
    page.set_execution_budget_for_testing(30);
    assert!(page
        .eval_target_expression(root, "(()=>{while(true){}})()", false, false)
        .await
        .is_err());
    page.set_execution_budget_for_testing(1000);
    assert!(
        !page
            .auxiliary_event_log()
            .info(&root.to_string())
            .unwrap()
            .closed
    );
    assert_eq!(
        page.eval_target_expression(root, "ownerMark===savedOwnerMark", false, true)
            .await
            .unwrap(),
        "true"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn history_physical_epochs_authenticate_actual_documents_across_group_restoration() {
    let server = fixture::Server::start().await;
    let log = NativeHistoryLog::default();
    let mut page = fixture::open(
        &server.origin,
        AuxiliaryHostServices {
            history_log: log.clone(),
            ..Default::default()
        },
    )
    .await;
    let root = page.auxiliary_root_id().unwrap();
    let original = log.snapshot(&root.to_string()).unwrap();
    assert_ne!(original["document"], "0");
    page.eval_target_expression(
        root,
        "history.pushState({same:true},'', '/same')",
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        log.snapshot(&root.to_string()).unwrap()["document"],
        original["document"]
    );
    let peer = fixture::child(&mut page).await;
    let peer_document = log.snapshot(&peer.to_string()).unwrap()["document"].clone();
    assert_ne!(peer_document, "0");
    assert_ne!(peer_document, original["document"]);
    page.set_content("<body>same document</body>", "load", None)
        .await
        .unwrap();
    page.eval_target_expression(root, "history.replaceState({same:true},'', '/same');child.savedHistory=history;child.savedGetter=Object.getOwnPropertyDescriptor(History.prototype,'state').get", false, false).await.unwrap();
    assert_eq!(
        log.snapshot(&root.to_string()).unwrap()["document"],
        original["document"]
    );
    page.navigate_target(root, &server.url("/replacement"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    let replacement = log.snapshot(&root.to_string()).unwrap();
    assert_ne!(replacement["document"], original["document"]);
    assert_ne!(replacement["document"], peer_document);
    assert_eq!(
        page.eval_target_expression(
            peer,
            "(()=>{try{savedGetter.call(savedHistory);return false}catch(error){return true}})()",
            false,
            true
        )
        .await
        .unwrap(),
        "true"
    );
    page.navigate_target(root, "", "back").await.unwrap();
    fixture::settle(&mut page).await;
    let restored = log.snapshot(&root.to_string()).unwrap();
    assert_eq!(restored["group"], original["group"]);
    assert_ne!(restored["document"], original["document"]);
    assert_ne!(restored["document"], replacement["document"]);
    assert_eq!(
        page.eval_target_expression(
            peer,
            "(()=>{try{savedGetter.call(savedHistory);return false}catch(error){return true}})()",
            false,
            true
        )
        .await
        .unwrap(),
        "true"
    );
    assert_eq!(
        log.snapshot(&peer.to_string()).unwrap()["document"],
        peer_document
    );
    page.close();
}
