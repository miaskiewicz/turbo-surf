//! Actual Chrome's authored cross-document insertion behavior.
#[tokio::test(flavor = "current_thread")]
async fn all_native_insertion_methods_match_chrome_foreign_identity_ranges_and_failures() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/dom-adoption-chrome155.json"
    ))
    .unwrap();
    let probe = include_str!("../../../../harness/detection/dom-adoption-probe.js")
        .trim()
        .trim_end_matches(';');
    let source = format!("(async()=>{{try{{globalThis.__RESULT=JSON.stringify(await({probe}));}}catch(error){{globalThis.__RESULT=JSON.stringify({{collectorError:error.name,message:error.message,stack:error.stack}});}}}})()");
    let actual = turbo_surf_render::eval_async(
        "<!doctype html><html><head></head><body>Authored adoption control</body></html>",
        "https://adoption.test/",
        &source,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(actual, fixture["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn retained_child_shadow_root_traversal_keeps_native_collection_indices() {
    let source = r#"(async()=>{
      const frame=document.createElement('iframe');
      const loaded=new Promise(resolve=>frame.onload=resolve);
      document.body.appendChild(frame);await loaded;
      const child=frame.contentWindow,doc=frame.contentDocument;
      const host=doc.createElement('div');doc.body.appendChild(host);
      const root=host.attachShadow({mode:'open'});root.innerHTML='<b>shadow</b>tail';
      const list=root.childNodes;
      globalThis.__RESULT=JSON.stringify({html:root.innerHTML,length:list.length,
        first:root.firstChild&&root.firstChild.nodeName,last:root.lastChild&&root.lastChild.nodeName,
        directLast:list[1]&&list[1].nodeName,itemLast:list.item(1)&&list.item(1).nodeName,
        array:Array.from(list).map(node=>node&&node.nodeName),
        ownsTraversal:['lastChild','previousSibling','isSameNode','isEqualNode'].every(name=>Object.hasOwn(child.Node.prototype,name)),
        previous:root.lastChild?.previousSibling===root.firstChild,
        equal:root.isEqualNode(root),same:root.isSameNode(root),
        own:Reflect.ownKeys(root)});
    })()"#;
    let result = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "https://adoption.test/",
        source,
    )
    .await
    .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["last"], "#text", "{result}");
    assert_eq!(result["directLast"], "#text", "{result}");
    assert_eq!(result["itemLast"], "#text", "{result}");
    assert_eq!(result["length"], 2, "{result}");
    for name in ["ownsTraversal", "previous", "equal", "same"] {
        assert_eq!(result[name], true, "{result}");
    }
    assert_eq!(result["own"], serde_json::json!([]), "{result}");
}

#[tokio::test(flavor = "current_thread")]
async fn variadic_conversion_matches_chrome_detachment_overlap_and_fragment_failures() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/dom-variadic-errors-chrome155.json"
    ))
    .unwrap();
    let probe = include_str!("../../../../harness/detection/dom-variadic-errors-probe.js")
        .trim()
        .trim_end_matches(';');
    let source = format!("(async()=>{{try{{globalThis.__RESULT=JSON.stringify(await({probe}));}}catch(error){{globalThis.__RESULT=JSON.stringify({{collectorError:error.name,message:error.message,stack:error.stack}});}}}})()");
    let actual = turbo_surf_render::eval_async(
        "<!doctype html><body>Authored insertion control</body>",
        "https://adoption.test/",
        &source,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(actual, fixture["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_native_insertions_retire_only_frames_detached_by_actual_conversion() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-insertion-errors-chrome155.json"
    ))
    .unwrap();
    let probe = include_str!("../../../../harness/detection/frame-insertion-errors-probe.js")
        .trim()
        .trim_end_matches(';');
    let source = format!("(async()=>{{try{{globalThis.__RESULT=JSON.stringify(await({probe}));}}catch(error){{globalThis.__RESULT=JSON.stringify({{collectorError:error.name,message:error.message,stack:error.stack}});}}}})()");
    let actual = turbo_surf_render::eval_async(
        "<!doctype html><body>Authored iframe insertion control</body>",
        "https://adoption.test/",
        &source,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(actual, fixture["result"]);
}
