#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_location_components_and_brand_checks_use_the_real_document_url() {
    let mut page = PageSession::open(
        "<!doctype html>",
        "https://example.test:8010/path/name?x=1#section",
        "",
        "",
        5_000,
    )
    .await
    .unwrap();
    let actual = page.eval_json_expression(r#"(() => {
        const loc=window.location;
        const failure=fn=>{try{fn();return 'accepted'}catch(error){return error.name}};
        const href=Object.getOwnPropertyDescriptor(loc,'href');
        const protocol=Object.getOwnPropertyDescriptor(loc,'protocol');
        return {
            components:[loc.href,loc.origin,loc.protocol,loc.host,loc.hostname,loc.port,loc.pathname,loc.search,loc.hash,String(loc)],
            failures:[
                failure(()=>new Location()),
                failure(()=>loc.assign()),
                failure(()=>loc.replace()),
                failure(()=>loc.assign.call({},'/unused')),
                failure(()=>loc.replace.call({},'/unused')),
                failure(()=>loc.reload.call({})),
                failure(()=>loc.toString.call({})),
                failure(()=>href.get.call({})),
                failure(()=>href.set.call({},'/unused')),
                failure(()=>protocol.get.call({})),
                failure(()=>protocol.set.call({},'http:')),
                failure(()=>loc.assign(Symbol('invalid'))),
                failure(()=>{loc.hash=Symbol('invalid')})
            ]
        };
    })()"#).await.unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(
        actual["components"],
        json!([
            "https://example.test:8010/path/name?x=1#section",
            "https://example.test:8010",
            "https:",
            "example.test:8010",
            "example.test",
            "8010",
            "/path/name",
            "?x=1",
            "#section",
            "https://example.test:8010/path/name?x=1#section"
        ])
    );
    assert_eq!(actual["failures"], json!(vec!["TypeError"; 13]));
    assert!(!page.has_pending_auxiliary_work());
}

#[tokio::test(flavor = "current_thread")]
async fn popup_is_top_level_while_its_real_frame_has_a_branded_ancestor_list() {
    let mut page = fixture::open("https://popup.test", Default::default()).await;
    let target = fixture::child(&mut page).await;
    let actual = page.eval_target_expression(target, r#"(() => {
        const frame=document.createElement('iframe');document.body.append(frame);
        const list=frame.contentWindow.location.ancestorOrigins;
        const failure=fn=>{try{fn();return 'accepted'}catch(error){return error.name}};
        return {
            top:location.ancestorOrigins.length,
            origins:[list.length,list[0],list.item(0),list.item(-1),list.item(999),list.contains('https://popup.test')],
            indexed:['0' in list,'999' in list,Object.keys(list).includes('0')],
            brand:Object.prototype.toString.call(list),
            failures:[failure(()=>new DOMStringList()),failure(()=>list.item()),failure(()=>list.contains()),
                failure(()=>list.item.call({},0)),failure(()=>list.contains.call({},'https://popup.test')),
                failure(()=>list.item(Symbol('index'))),failure(()=>list.contains(Symbol('origin')))]
        };
    })()"#, false, true).await.unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(actual["top"], 0);
    assert_eq!(
        actual["origins"],
        json!([
            1,
            "https://popup.test",
            "https://popup.test",
            null,
            null,
            true
        ])
    );
    assert_eq!(actual["indexed"], json!([true, false, true]));
    assert_eq!(actual["brand"], "[object DOMStringList]");
    assert_eq!(actual["failures"], json!(vec!["TypeError"; 7]));
}

#[tokio::test(flavor = "current_thread")]
async fn cross_origin_location_rejects_reads_but_replace_uses_the_actual_caller_origin() {
    let source = fixture::Server::start().await;
    let destination = fixture::Server::start().await;
    let mut page = fixture::open(&source.origin, Default::default()).await;
    let target = fixture::child(&mut page).await;
    page.navigate_target(target, &destination.url("/cross"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    let actual = page
        .eval_json_expression(
            r#"(() => {
        const loc=child.location;
        const failure=fn=>{try{fn();return 'accepted'}catch(error){return error.name}};
        return {
            readableMethod:typeof loc.replace,
            prototype:Object.getPrototypeOf(loc)===null,
            failures:[failure(()=>child.document),failure(()=>loc.href),failure(()=>loc.origin),
                failure(()=>loc.assign),failure(()=>loc.reload),failure(()=>String(loc))],
            mutation:[failure(()=>Object.defineProperty(loc,'href',{value:'ignored'})),
                failure(()=>Object.setPrototypeOf(loc,{}))],
            owns:['href' in loc,Reflect.ownKeys(loc).includes('replace')]
        };
    })()"#,
        )
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(actual["readableMethod"], "function");
    assert_eq!(actual["prototype"], true);
    assert_eq!(actual["failures"], json!(vec!["SecurityError"; 6]));
    assert_eq!(actual["mutation"], json!(vec!["SecurityError"; 2]));
    assert_eq!(actual["owns"], json!([true, true]));
    page.eval("child.location.replace('/returned');")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        page.eval_expression("child.document.title").await.unwrap(),
        "returned"
    );
    assert_eq!(source.requests().last().unwrap().target, "/returned");
}
