//! A changed inline stylesheet must not revisit unrelated document resources.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn inline_style_insertions_do_not_rescan_existing_resource_elements() {
    let mut page = PageSession::open(
        "<link id=sentinel rel=stylesheet><main id=box></main>",
        "http://localhost/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = page.eval_json_expression(r#"(() => {
        let reads = 0;
        Object.defineProperty(document.querySelector('#sentinel'), 'disabled', {get(){reads++;return false;}});
        let last;
        for(let i=0;i<100;i++) {
            last=document.createElement('style');
            last.textContent='#box{width:'+i+'px}';
            document.head.appendChild(last);
        }
        const afterInsert=reads;
        last.firstChild.data='#box{width:120px}';
        last.textContent='#box{width:130px}';
        last.setAttribute('media','screen');
        return [afterInsert,reads,last.sheet.cssRules[0].style.width];
    })()"#).await.unwrap();
    assert_eq!(result, r#"[0,0,"130px"]"#);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn mixed_insertions_and_nested_resources_retain_full_admission() {
    let mut page = PageSession::open(
        "<link id=sentinel rel=stylesheet>",
        "http://localhost/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result=page.eval_json_expression(r#"(() => {
        let reads=0;
        Object.defineProperty(document.querySelector('#sentinel'),'disabled',{get(){reads++;return false;}});
        const fragment=document.createDocumentFragment();
        fragment.appendChild(document.createElement('style'));
        fragment.appendChild(document.createElement('link'));
        document.head.appendChild(fragment);
        const mixed=reads>0;
        reads=0;
        const style=document.createElement('style');
        const nested=document.createElement('link');
        nested.rel='stylesheet';
        style.appendChild(nested);
        document.head.appendChild(style);
        const inserted=reads>0;
        reads=0;
        nested.setAttribute('media','screen');
        return [mixed,inserted,reads>0];
    })()"#).await.unwrap();
    assert_eq!(result, "[true,true,true]");
    page.close();
}
