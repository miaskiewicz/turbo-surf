//! Public layout and geometry contracts: authored DOM inputs, no private state edits.
use turbo_surf_render::PageSession;

async fn page(html: &str) -> PageSession {
    PageSession::open(html, "https://geometry.test/", "", "", 5_000)
        .await
        .expect("open real native page")
}

#[tokio::test]
async fn hidden_text_range_has_no_placed_boxes() {
    let mut session = page(
        "<p id='hidden' style='display:none'>invisible text</p><p id='visible'>visible text</p>",
    )
    .await;
    let result = session
        .eval_json_expression(
            r#"(() => {
        const before = document.body.innerHTML;
        const range = document.createRange();
        range.selectNodeContents(document.getElementById('hidden'));
        const bounds = range.getBoundingClientRect();
        const visible = document.createRange();
        visible.selectNodeContents(document.getElementById('visible'));
        const placed = visible.getBoundingClientRect();
        return [range.getClientRects().length, bounds.x, bounds.y, bounds.width, bounds.height,
            visible.getClientRects().length > 0, placed.height > 0,
            document.body.innerHTML === before, document.getElementById('hidden').textContent,
            document.getElementById('visible').textContent];
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        result,
        "[0,0,0,0,0,true,true,true,\"invisible text\",\"visible text\"]"
    );
}

#[tokio::test]
async fn positioned_scroll_target_moves_container_but_fixed_target_does_not() {
    let mut session = page(
        r#"<style>
        #port{width:160px;height:100px;overflow:auto;position:relative}
        #space{width:800px;height:800px;position:relative}
        #absolute{position:absolute;top:500px;left:500px;width:20px;height:20px}
        #fixed{position:fixed;top:20px;left:20px;width:20px;height:20px}
        </style><div id='port'><div id='space'><div id='absolute'></div>
        <div id='fixed'></div></div></div>"#,
    )
    .await;
    let result = session
        .eval_json_expression(
            r#"(() => {
        const port = document.getElementById('port');
        document.getElementById('absolute').scrollIntoView({block:'nearest',inline:'nearest'});
        const moved = port.scrollTop > 0 && port.scrollLeft > 0;
        port.scrollTo(0, 0);
        document.getElementById('fixed').scrollIntoView({block:'nearest',inline:'nearest'});
        return [moved, port.scrollTop, port.scrollLeft];
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(result, "[true,0,0]");
}

#[tokio::test]
async fn explicit_viewport_options_set_actual_window_and_root_dimensions() {
    let mut session = PageSession::open_with_document_response_and_options(
        "<style>html,body{margin:0}</style><p>viewport</p>",
        "https://geometry.test/viewport",
        "",
        "",
        5_000,
        "",
        Default::default(),
        &[],
        Default::default(),
        Default::default(),
        None,
        turbo_surf_render::PageSessionOptions {
            viewport: Some([640, 480]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let result = session.eval_json_expression(
        "[window.innerWidth, window.innerHeight, document.documentElement.clientWidth, document.documentElement.clientHeight]",
    ).await.unwrap();
    assert_eq!(result, "[640,480,640,480]");
}

#[tokio::test]
async fn cached_own_none_admission_keeps_visible_and_mutated_style_elements_on_normal_geometry() {
    let mut session =
        page(r#"<!doctype html><body><style id=subject>/* default none */</style>body</body>"#)
            .await;
    let actual=session.eval_json_expression(r#"(()=>{
      const element=document.getElementById('subject');
      const read=()=>{getComputedStyle(element).position;const r=element.getBoundingClientRect();return [r.width,r.height,element.getClientRects().length]};
      const results=[read()];
      element.style.cssText='display:block;width:40px;height:20px';results.push(read());
      element.style.cssText='display:none';results.push(read());
      document.styleSheets[0].insertRule('#subject{display:block!important;width:31px;height:17px}',0);results.push(read());
      let brand=false;try{Element.prototype.getBoundingClientRect.call({})}catch(error){brand=error instanceof TypeError;}
      return {results,brand};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!({"results":[[0,0,0],[40,20,1],[0,0,0],[31,17,1]],"brand":true})
    );
}
