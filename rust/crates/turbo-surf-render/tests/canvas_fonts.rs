//! Canvas `measureText` resolves `ctx.font` against the page's own font registry (the
//! system faces live layout uses; turbo-dom-browser-env `set_canvas_fonts`,
//! miaskiewicz/turbo-dom#237), so with no host measurer installed a canvas run measures
//! like the same run laid out in the document, as in Chrome.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn canvas_measure_text_uses_the_page_fonts() {
    let mut page = PageSession::open(
        "<!doctype html><body><span id=s style='font:20px sans-serif'>Hello world</span>",
        "https://example.test/",
        "",
        "",
        15000,
    )
    .await
    .unwrap();
    let value = page
        .eval_expression(
            r#"(()=>{
  const ctx=document.createElement('canvas').getContext('2d');
  ctx.font='20px sans-serif';
  const canvas=ctx.measureText('Hello world').width;
  const span=document.getElementById('s').getBoundingClientRect().width;
  return JSON.stringify([canvas,span]);
})()"#,
        )
        .await
        .unwrap();
    let [canvas, span]: [f64; 2] = serde_json::from_str(&value).unwrap();
    assert!(span > 0.0, "{value}");
    // The document rounds advances to 1/64 px LayoutUnits; the canvas keeps Blink's 16.16.
    assert!((canvas - span).abs() < 0.05, "canvas {canvas} vs layout {span}");
    page.close();
}
