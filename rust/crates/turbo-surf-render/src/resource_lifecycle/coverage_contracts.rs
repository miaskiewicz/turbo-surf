//! Native resource authority contracts over real documents, not guest capabilities.
use super::*;

async fn page() -> PageSession {
    PageSession::open(
        "<body><iframe id=same srcdoc='<body>'></iframe>\
         <iframe id=opaque sandbox=allow-scripts srcdoc='<body>'></iframe>\
         <iframe id=blocked sandbox=allow-same-origin srcdoc='<body>'></iframe>",
        "https://resource-authority.test/root",
        "",
        "",
        10_000,
    )
    .await
    .unwrap()
}

fn root_context(page: &mut PageSession) -> v8::Global<v8::Context> {
    private_infrastructure::context(&mut page.rt).unwrap()
}

fn child_context(page: &mut PageSession, kind: &str) -> v8::Global<v8::Context> {
    let contexts = page
        .rt
        .op_state()
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    deno_core::scope!(scope, page.rt);
    contexts
        .into_iter()
        .find(|candidate| {
            let context = v8::Local::new(scope, candidate);
            let Some(host) = context
                .get_slot::<FrameHost>()
                .and_then(|host| host.0.to_local(scope))
            else {
                return false;
            };
            crate::browser_env::node_attribute(scope, host, "id").as_deref() == Some(kind)
        })
        .expect("real frame registry must contain the requested child")
}

fn document(page: &mut PageSession, context: &v8::Global<v8::Context>) -> v8::Global<v8::Object> {
    deno_core::scope!(scope, page.rt);
    let context = v8::Local::new(scope, context);
    let document = context
        .get_slot::<ActiveDocument>()
        .expect("actual frame document")
        .0
        .to_local(scope)
        .expect("live native document wrapper");
    v8::Global::new(scope, document)
}

fn owner_error(
    page: &mut PageSession,
    caller: &v8::Global<v8::Context>,
    document: &v8::Global<v8::Object>,
) -> Option<String> {
    deno_core::scope!(scope, page.rt);
    let caller = v8::Local::new(scope, caller);
    let scope = &mut v8::ContextScope::new(scope, caller);
    assert_eq!(
        scope.get_entered_or_microtask_context(),
        caller,
        "native authority proof must enter the actual caller realm"
    );
    let document = v8::Local::new(scope, document);
    owner_context(scope, document)
        .err()
        .map(|error| error.to_string())
}

#[tokio::test(flavor = "current_thread")]
async fn native_resource_owner_accepts_live_documents_and_rejects_real_authority_boundaries() {
    let mut page = page().await;
    let _activation = page.rt.activation_key().enter();
    let root = root_context(&mut page);
    let root_document = document(&mut page, &root);
    let same = child_context(&mut page, "same");
    let same_document = document(&mut page, &same);
    let opaque = child_context(&mut page, "opaque");
    let opaque_document = document(&mut page, &opaque);
    let blocked = child_context(&mut page, "blocked");
    let blocked_document = document(&mut page, &blocked);

    assert_eq!(owner_error(&mut page, &root, &root_document), None);
    assert_eq!(owner_error(&mut page, &root, &same_document), None);
    assert_eq!(
        owner_error(&mut page, &root, &opaque_document).as_deref(),
        Some("script document is not same-origin")
    );
    assert_eq!(
        owner_error(&mut page, &root, &blocked_document).as_deref(),
        Some("script execution is blocked by the iframe sandbox")
    );
    assert_eq!(
        page.eval_json_expression("({root:document.body.localName,same:document.querySelector('#same').contentDocument.body.localName})")
            .await.unwrap(),
        r#"{"root":"body","same":"body"}"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_resource_owner_rejects_a_retained_removed_document_without_retiring_root() {
    let mut page = page().await;
    let _activation = page.rt.activation_key().enter();
    let root = root_context(&mut page);
    let same = child_context(&mut page, "same");
    let retained = document(&mut page, &same);
    assert_eq!(owner_error(&mut page, &root, &retained), None);
    page.eval_expression("document.querySelector('#same').remove(); true")
        .await
        .unwrap();
    {
        deno_core::scope!(scope, page.rt);
        let child = v8::Local::new(scope, &same);
        assert!(child.get_slot::<FrameRetired>().is_some());
    }
    assert_eq!(
        owner_error(&mut page, &root, &retained).as_deref(),
        Some("iframe document has been replaced")
    );
    assert_eq!(
        page.eval_expression("document.body.localName")
            .await
            .unwrap(),
        "body"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_resource_owner_uses_the_actual_opaque_caller_instead_of_the_parent() {
    let mut page = page().await;
    let _activation = page.rt.activation_key().enter();
    let root = root_context(&mut page);
    let opaque = child_context(&mut page, "opaque");
    let root_document = document(&mut page, &root);
    let opaque_document = document(&mut page, &opaque);
    assert_eq!(owner_error(&mut page, &opaque, &opaque_document), None);
    assert_eq!(
        owner_error(&mut page, &opaque, &root_document).as_deref(),
        Some("script document is not same-origin")
    );
    assert_eq!(
        owner_error(&mut page, &root, &opaque_document).as_deref(),
        Some("script document is not same-origin")
    );
    assert_eq!(
        page.eval_expression("document.body.localName")
            .await
            .unwrap(),
        "body"
    );
}
