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
