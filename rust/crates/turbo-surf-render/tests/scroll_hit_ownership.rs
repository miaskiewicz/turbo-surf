//! Public native scroll and hit contracts for turbo-dom release fixes #550/#551.
use turbo_surf_render::PageSession;

async fn page(html: &str) -> PageSession {
    PageSession::open(html, "https://scroll-hit.test/", "", "", 5000)
        .await
        .unwrap()
}

#[tokio::test]
async fn containment_keeps_body_scrolling_on_its_own_scrollport() {
    for (root, body) in [("", "contain:paint"), ("contain:paint", "")] {
        let html=format!("<!doctype html><html style='{root}'><head><style>body{{margin:8px;height:40px;overflow:hidden;{body}}}button{{height:30px}}</style></head><body><div style='height:100px'></div><button id=target>Target</button></body></html>");
        let mut session = page(&html).await;
        let actual=session.eval_json_expression("(()=>{document.getElementById('target').scrollIntoViewIfNeeded();return [document.body.scrollTop,document.documentElement.scrollTop]})()").await.unwrap();
        assert_eq!(actual, "[90,0]", "root={root} body={body}");
        session.close();
    }
}

#[tokio::test]
async fn hit_testing_preserves_viewport_overflow_and_fixed_clip_ownership() {
    let inputs=[
      "<!doctype html><html><head><style>body{margin:8px;height:40px;overflow:hidden}#space{height:100px}#root{position:relative;width:200px;height:56px}label{display:block;height:20px}#control{height:36px}</style></head><body><div id=space></div><div id=root><label>Schedule</label><div id=control role=combobox>Control</div></div></body></html>",
      "<!doctype html><html><head><style>body{margin:8px}#clip{height:40px;overflow:clip}#root{position:fixed;top:300px;width:200px;height:56px}label{display:block;height:20px}#control{height:36px}</style></head><body><div id=clip><div id=root><label>Schedule</label><div id=control role=combobox>Control</div></div></div></body></html>",
    ];
    for html in inputs {
        let mut session = page(html).await;
        let actual=session.eval_json_expression("(()=>{const r=document.getElementById('root').getBoundingClientRect();return {positive:r.width===200&&r.height===56,hit:document.elementFromPoint(r.left+r.width/2,r.top+r.height/2)?.id}})()").await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
            serde_json::json!({"positive":true,"hit":"control"})
        );
        session.close();
    }
}
