//! Guest/default-action integration, independent of the driver click fallback.
use turbo_surf_render::{AuxiliaryPageEvent, PageSession};

#[tokio::test]
async fn guest_anchor_activation_and_driver_receipt_create_one_actual_child() {
    let mut page = PageSession::open(
        "<!doctype html><a id=a href='about:blank' target=_blank><span id=inner>Open</span></a>",
        "https://popup.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let log = page.auxiliary_event_log();
    log.take();
    page.eval("document.getElementById('a').click();")
        .await
        .unwrap();
    let events = log.take();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AuxiliaryPageEvent::Created { .. }))
            .count(),
        1
    );
    let child = log
        .pages()
        .into_iter()
        .find(|child| child.source.is_some())
        .unwrap();
    assert_eq!(
        page.eval_target_expression(
            child.target.parse().unwrap(),
            "String(window.opener === null)",
            false,
            false
        )
        .await
        .unwrap(),
        "true"
    );
    let receipt = page.eval_engine("const node=document.getElementById('inner'); const event=new MouseEvent('click',{bubbles:true,cancelable:true,button:0}); node.dispatchEvent(event); globalThis.__RESULT=String(__tsNs.dispatchAnchorDefault(node,event));").await.unwrap();
    assert_eq!(receipt, "true");
    assert_eq!(
        log.take()
            .iter()
            .filter(|event| matches!(event, AuxiliaryPageEvent::Created { .. }))
            .count(),
        1
    );
    assert_eq!(log.pages().len(), 3);
}

#[tokio::test]
async fn prevented_guest_anchor_has_no_native_page_or_handled_receipt() {
    let mut page = PageSession::open(
        "<!doctype html><a id=a href='about:blank' target=_blank>Open</a>",
        "https://popup.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let log = page.auxiliary_event_log();
    log.take();
    let receipt = page.eval_engine("const node=document.getElementById('a'); node.addEventListener('click',e=>e.preventDefault()); const event=new MouseEvent('click',{bubbles:true,cancelable:true,button:0}); node.dispatchEvent(event); globalThis.__RESULT=String(__tsNs.dispatchAnchorDefault(node,event));").await.unwrap();
    assert_eq!(receipt, "false");
    assert!(log
        .take()
        .iter()
        .all(|event| !matches!(event, AuxiliaryPageEvent::Created { .. })));
    assert_eq!(log.pages().len(), 1);
}
