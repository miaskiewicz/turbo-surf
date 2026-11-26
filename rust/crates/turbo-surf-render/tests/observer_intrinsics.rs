use turbo_surf_render::PageSession;

async fn observe(html: &str, expression: &str) -> String {
    let mut page = PageSession::open(html, "https://observer.test/", "", "", 5000)
        .await
        .unwrap();
    assert!(!page.hydration_diagnostics().budget_exceeded);
    let result = page.eval_expression(expression).await.unwrap();
    page.close();
    result
}

#[tokio::test(flavor = "current_thread")]
async fn observer_css_reads_bypass_author_get_property_value() {
    let html = r#"<!doctype html><style>#box{width:100px;height:20px;padding:5px}</style>
      <body><div id=box></div><script>
      globalThis.authorCalls=0;globalThis.observed=[];
      const original=CSSStyleDeclaration.prototype.getPropertyValue;
      CSSStyleDeclaration.prototype.getPropertyValue=function(name){
        authorCalls++;return original.call(this,name);
      };
      const ro=new ResizeObserver(entries=>{
        for(const e of entries)observed.push([e.contentRect.width,e.contentRect.height]);
      });
      ro.observe(document.getElementById('box'));
      requestAnimationFrame(()=>requestAnimationFrame(()=>ro.disconnect()));
      </script>"#;
    assert_eq!(
        observe(html, "JSON.stringify({observed,authorCalls})").await,
        r#"{"observed":[[100,20]],"authorCalls":0}"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn observer_callback_mutation_reaches_the_following_delivery() {
    let html = r#"<!doctype html><style>#box{width:100px;height:20px;padding:5px}</style>
      <body><div id=box></div><script>
      globalThis.observed=[];const box=document.getElementById('box');
      const a=new ResizeObserver(entries=>{
        for(const e of entries)observed.push(['a',e.contentRect.width,e.borderBoxSize[0].inlineSize]);
        if(observed.length===1)box.style.width='140px';
      });
      const b=new ResizeObserver(entries=>{
        for(const e of entries)observed.push(['b',e.contentRect.width,e.borderBoxSize[0].inlineSize]);
      });
      a.observe(box);b.observe(box);
      requestAnimationFrame(()=>requestAnimationFrame(()=>requestAnimationFrame(()=>{a.disconnect();b.disconnect();})));
      </script>"#;
    assert_eq!(
        observe(html, "JSON.stringify(observed)").await,
        r#"[["a",100,110],["b",100,110],["a",140,150],["b",140,150]]"#
    );
}
