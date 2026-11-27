use serde_json::{json, Value};
use turbo_surf_render::PageSession;

// Same filter + transform keyframes as Next's devtools issues badge.
const CSS: &str = "@keyframes fadeOut{0%{opacity:1;filter:blur(0px);transform:translateY(0)}100%{opacity:0;transform:translateY(-12px);filter:blur(2px)}}#overlay{width:10px;height:10px;animation:fadeOut 100s forwards}#target{width:120px;height:20px}";

async fn probe(shadow: bool) -> Value {
    let html = format!("<!doctype html><style>{CSS}</style><aside id=overlay></aside><main id=target></main><div id=host></div>");
    let source = format!(
        r#"(() => {{
      let overlay=document.getElementById('overlay');
      if({shadow}) {{
        const root=document.getElementById('host').attachShadow({{mode:'open'}});
        root.innerHTML='<style>'+{css}+'</style>';
        root.appendChild(overlay);
      }}
      const target=document.getElementById('target');
      const read=node=>{{try{{return {{width:node.getBoundingClientRect().width}};}}catch(error){{return {{error:error.message}};}}}};
      const independent=read(target);
      const own=read(overlay);
      let scroll;
      try {{ scroll={{height:document.body.scrollHeight}}; }} catch(error) {{ scroll={{error:error.message}}; }}
      overlay.appendChild(target);
      return JSON.stringify({{independent,own,scroll,ancestor:read(target)}});
    }})()"#,
        css = serde_json::to_string(CSS).unwrap()
    );
    let mut page = PageSession::open(&html, "http://127.0.0.1/", "", "", 10000)
        .await
        .unwrap();
    let result = page
        .eval_engine(&format!("globalThis.__RESULT = {source};"))
        .await
        .unwrap();
    page.close();
    serde_json::from_str(&result).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn filter_and_transform_animation_preserves_an_independent_branch_box() {
    for shadow in [false, true] {
        let result = probe(shadow).await;
        assert_eq!(
            result["independent"],
            json!({"width":120}),
            "shadow={shadow}: {result}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn filter_animation_keeps_own_and_descendant_geometry_strict() {
    for shadow in [false, true] {
        let result = probe(shadow).await;
        for phase in ["own", "ancestor", "scroll"] {
            assert!(
                result[phase]["error"]
                    .as_str()
                    .unwrap()
                    .contains("animation resource"),
                "{phase}: {result}"
            );
        }
    }
}
