use sha2::{Digest, Sha256};
#[tokio::test(flavor = "current_thread")]
async fn live_layout_neutral_admission_matches_real_chrome_and_rejects_used_dependencies() {
    let probe = include_str!("../../../../harness/detection/neutral-at-rules-probe.js");
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/neutral-at-rules-chrome155.json"
    ))
    .unwrap();
    assert_eq!(fixture["browser"], "Chrome/155.0.8059.80");
    assert_eq!(
        fixture["probeSha256"],
        format!("{:x}", Sha256::digest(probe.as_bytes()))
    );
    assert_eq!(fixture["result"][1]["box"], serde_json::json!([70, 10]));
    assert_eq!(fixture["result"][2]["box"], serde_json::json!([70, 10]));
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><head></head><body>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    let rows = actual.as_array().unwrap();
    let expected = fixture["result"].as_array().unwrap();
    assert_eq!(rows.len(), expected.len());
    for (row, control) in rows.iter().zip(expected) {
        let name = control["name"].as_str().unwrap();
        assert_eq!(row["name"], name);
        if matches!(
            name,
            "absent-named-container"
                | "paint-registration"
                | "transitive-paint-registration"
                | "view-transition-auto"
                | "view-transition-none"
        ) {
            assert_eq!(row, control, "{name}");
        } else {
            let message = row["message"]
                .as_str()
                .unwrap_or_else(|| panic!("unsupported dependency accepted: {name}: {row}"));
            assert!(
                message.contains("container query inactive")
                    || message.contains("geometry dependency")
                    || message.contains("rule type 0"),
                "{name}: {message}"
            );
        }
    }
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn geometry_admission_rechecks_inline_and_stylesheet_dependencies_after_cache_hits() {
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><style id='rules'>#target{width:10px;height:10px}@container probe-container (max-width:200px){#target{width:70px}}</style><div id='parent'><div id='target'></div></div>",
        "https://example.test/", "", "", 5000,
    ).await.unwrap();
    let raw = page.eval_json_expression(r#"(()=>{
      const parent=document.getElementById('parent'),target=document.getElementById('target'),sheet=document.getElementById('rules').sheet;
      const read=()=>{try{return target.getBoundingClientRect().width}catch(error){return error.message}};
      const values=[read(),read()];
      parent.setAttribute('style','container:probe-container / inline-size;width:100px');values.push(read());
      parent.removeAttribute('style');values.push(read());
      sheet.insertRule('#parent{container:probe-container / inline-size;width:100px}',sheet.cssRules.length);values.push(read());
      sheet.deleteRule(sheet.cssRules.length-1);values.push(read());return values;
    })()"#).await.unwrap();
    let values: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for i in [0, 1, 3, 5] {
        assert_eq!(values[i], 10);
    }
    for i in [2, 4] {
        assert!(values[i]
            .as_str()
            .unwrap()
            .contains("container query inactive"));
    }
    page.close();
}
