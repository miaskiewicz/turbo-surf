use super::*;

async fn page() -> PageSession {
    PageSession::open(
        "<title>history</title>",
        "https://history.test/start",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

fn record(page: &mut PageSession, source: &str) -> v8::Global<v8::Value> {
    let target = page.auxiliary_root_id().unwrap();
    let _activation = page.rt.activation_key().enter();
    auxiliary_pages::execute_target(&mut page.rt, AuxiliaryContextId(target), source, false)
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn metadata_records_reject_executable_properties_without_invoking_them() {
    let mut page = page().await;
    record(
        &mut page,
        "globalThis.historyTrapCalls=0;globalThis.historyGetterCalls=0;null",
    );
    for (source, expected) in [
        (
            "new Proxy({url:'safe'}, {getOwnPropertyDescriptor(){historyTrapCalls++;throw Error('trap ran')}})",
            "own data record",
        ),
        (
            "({get url(){historyGetterCalls++;throw Error('getter ran')}})",
            "accessors are not accepted",
        ),
        ("Object.create({url:'inherited'})", "incomplete"),
        ("({url:42})", "not a string"),
    ] {
        let value = record(&mut page, source);
        {
            let _activation = page.rt.activation_key().enter();
            deno_core::scope!(scope, page.rt);
            let value = v8::Local::new(scope, value);
            let object = v8::Local::<v8::Object>::try_from(value).unwrap();
            let error = text(scope, object, "url", 32).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert!(!scope.is_execution_terminating());
        }
        let target = page.auxiliary_root_id().unwrap();
        assert_eq!(page.eval_target_expression(target, "[historyTrapCalls,historyGetterCalls]", false, true).await.unwrap(), "[0,0]");
    }
    let target = page.auxiliary_root_id().unwrap();
    assert_eq!(
        page.eval_target_expression(
            target,
            "[document.title,history.length,location.pathname]",
            false,
            true
        )
        .await
        .unwrap(),
        "[\"history\",1,\"/start\"]"
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn metadata_string_budget_counts_encoded_bytes_and_accepts_exact_boundary() {
    let mut page = page().await;
    for (source, budget, expected) in [
        ("({url:'abcd'})", 3, None),
        ("({url:'éé'})", 3, None),
        ("({url:'éé'})", 4, Some("éé")),
        ("({url:''})", 0, Some("")),
    ] {
        let value = record(&mut page, source);
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, page.rt);
        let value = v8::Local::new(scope, value);
        let object = v8::Local::<v8::Object>::try_from(value).unwrap();
        let actual = text(scope, object, "url", budget);
        match expected {
            Some(expected) => assert_eq!(actual.unwrap(), expected),
            None => assert!(actual.unwrap_err().to_string().contains("byte budget")),
        }
    }
    page.close();
}
