//! Debugger source capture is opt-in and must retain dynamically compiled programs.
use serde_json::Value;
use turbo_surf_render::{stable_window_proxy, PageSession};

#[tokio::test(flavor = "current_thread")]
async fn debugger_inventory_captures_real_eval_function_and_child_sources() {
    let directory =
        std::env::temp_dir().join(format!("turbo-native-inventory-{}", std::process::id()));
    std::env::set_var("TURBO_SURF_JS_TRACE", &directory);
    let html = r#"<body><script>
      eval('globalThis.dynamicValue=42;\n//# sourceURL=eval-fixture.js');
      globalThis.functionValue=Function('return 73;\n//# sourceURL=function-fixture.js')();
      </script><iframe srcdoc="&lt;script&gt;globalThis.childValue=Function('return 19;\n//# sourceURL=child-function.js')()&lt;/script&gt;"></iframe></body>"#;
    let mut page = PageSession::open(html, "https://inventory.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval("globalThis.__RESULT=JSON.stringify([dynamicValue,functionValue,document.querySelector('iframe').contentWindow.childValue])").await.unwrap(), "[42,73,19]");
    let mut peer = PageSession::open("<body>peer", "https://peer.test/", "", "", 5000)
        .await
        .unwrap();
    page.close();
    assert_eq!(
        peer.eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "peer"
    );
    peer.close();
    std::env::remove_var("TURBO_SURF_JS_TRACE");
    let inventory: Value =
        serde_json::from_slice(&std::fs::read(directory.join("isolate-0/inventory.json")).unwrap())
            .unwrap();
    assert_eq!(inventory["dropped"], 0);
    assert_eq!(inventory["pendingSources"], 0);
    assert!(
        inventory["errors"].as_array().unwrap().is_empty(),
        "{inventory}"
    );
    let scripts = inventory["scripts"].as_array().unwrap();
    for name in ["eval-fixture.js", "function-fixture.js"]
        .into_iter()
        .chain(stable_window_proxy().then_some("child-function.js"))
    {
        let script = scripts
            .iter()
            .find(|row| row["url"] == name)
            .unwrap_or_else(|| panic!("missing {name}: {inventory}"));
        let source = std::fs::read_to_string(
            directory
                .join("isolate-0")
                .join(script["sourceFile"].as_str().unwrap()),
        )
        .unwrap();
        assert!(source.contains(name), "{source}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.join("isolate-0/inventory.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
    verify_child_init_and_snapshots().await;
}

async fn verify_child_init_and_snapshots() {
    let mut page=PageSession::open_with_init_script("<body><iframe srcdoc='<body></body>'></iframe></body>","https://interfaces.test/","","",5000,r#"globalThis.hostInitialized=true;if(!globalThis[Symbol.for('turbo-surf.interface-trace')])Object.defineProperty(globalThis,Symbol.for('turbo-surf.interface-trace'),{value:{snapshot:()=>({accesses:[{name:'fixture',origin:'page',kind:'call',count:1}],installed:[],skipped:[],dropped:0})}});"#).await.unwrap();
    assert_eq!(page.eval("globalThis.__RESULT=String(document.querySelector('iframe').contentWindow.hostInitialized)").await.unwrap(),"true");
    let frames = page.frame_interface_snapshots();
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["url"], "about:srcdoc");
    assert_eq!(frames[0]["interfaces"]["accesses"][0]["name"], "fixture");
}
