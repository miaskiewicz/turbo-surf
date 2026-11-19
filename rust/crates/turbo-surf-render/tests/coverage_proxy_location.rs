//! Public cross-origin descriptor, key and Location contracts on the default engine.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn opaque_iframe_window_descriptors_preserve_access_and_receiver_checks() {
    let mut page = PageSession::open("<body>", "https://proxy-contract.test/", "", "", 5_000)
        .await
        .unwrap();
    let actual = page.eval_json_expression(r#"(async()=>{
      const same=document.createElement('iframe');
      const sameReady=new Promise(resolve=>same.onload=resolve);
      same.srcdoc='<body>same-origin';document.body.append(same);await sameReady;
      const sameWindow=same.contentWindow;
      const frame=document.createElement('iframe');
      frame.setAttribute('sandbox','allow-scripts');
      const ready=new Promise(resolve=>frame.onload=resolve);
      frame.srcdoc='<body><iframe></iframe>';
      document.body.append(frame);await ready;
      const child=frame.contentWindow;
      const failure=fn=>{try{fn();return 'accepted'}catch(error){return error.name}};
      const closedGetter=Object.getOwnPropertyDescriptor(sameWindow,'closed').get;
      const sameChecks=[closedGetter.call(sameWindow)===false,
        failure(()=>closedGetter.call({})),failure(()=>sameWindow.Window()),
        failure(()=>new sameWindow.Window())];
      same.remove();sameChecks.push(closedGetter.call(sameWindow)===true);
      const accessors=['window','self','frames','closed','length','top','opener','parent','location'];
      const descriptorRows=accessors.map(key=>{
        const first=Object.getOwnPropertyDescriptor(child,key);
        const second=Object.getOwnPropertyDescriptor(child,key);
        return [key,typeof first.get,first.get===second.get,
          first.get.call(child)===child[key],first.enumerable,first.configurable,
          failure(()=>first.get.call({})),
          key==='location'?failure(()=>first.set.call({},'/never')):'no setter'];
      });
      const methods=['postMessage','blur','focus','close','then'].map(key=>{
        const descriptor=Object.getOwnPropertyDescriptor(child,key);
        return [key,descriptor.value===child[key],descriptor.writable,
          descriptor.enumerable,descriptor.configurable];
      });
      const symbols=[Symbol.toStringTag,Symbol.hasInstance,Symbol.isConcatSpreadable];
      const symbolRows=symbols.map(key=>{
        const descriptor=Object.getOwnPropertyDescriptor(child,key);
        return [child[key]===undefined,descriptor.value===undefined,
          descriptor.writable,descriptor.enumerable,descriptor.configurable,key in child];
      });
      const keys=Reflect.ownKeys(child);
      return {sameChecks,descriptorRows,methods,symbolRows,
        topology:[child.window===child,child.self===child,child.frames===child,
          child.parent===window,child.top===window,child.length===1,
          child[0]===child.frames[0],child.closed===false,child.opener===null],
        keys:[keys.includes('0'),accessors.every(key=>keys.includes(key)),
          symbols.every(key=>keys.includes(key))],
        failures:[failure(()=>child.document),failure(()=>('document' in child)),
          failure(()=>Object.getOwnPropertyDescriptor(child,'document')),
          failure(()=>child[99]),failure(()=>child[Symbol.iterator]),
          failure(()=>Object.getOwnPropertyDescriptor(child,Symbol.iterator)),
          failure(()=>{child.arbitrary=1}),failure(()=>child.postMessage())],
        harmless:[child.blur(),child.focus(),child.close()].every(value=>value===undefined)};
    })()"#).await.unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(
        actual["sameChecks"],
        json!([true, "TypeError", "TypeError", "TypeError", true])
    );
    for row in actual["descriptorRows"].as_array().unwrap() {
        assert_eq!(row[1], "function", "{row}");
        assert_eq!(row[2], true, "{row}");
        assert_eq!(row[3], true, "{row}");
        assert_eq!(row[4], false, "{row}");
        assert_eq!(row[5], true, "{row}");
        assert_eq!(row[6], "TypeError", "{row}");
        assert_eq!(
            row[7],
            if row[0] == "location" {
                "TypeError"
            } else {
                "no setter"
            },
            "{row}"
        );
    }
    for row in actual["methods"].as_array().unwrap() {
        assert_eq!(
            &row.as_array().unwrap()[1..],
            &[json!(true), json!(false), json!(false), json!(true)],
            "{row}"
        );
    }
    assert_eq!(
        actual["symbolRows"],
        json!(vec![vec![true, true, false, false, true, true]; 3])
    );
    assert_eq!(actual["topology"], json!(vec![true; 9]));
    assert_eq!(actual["keys"], json!([true, true, true]));
    assert_eq!(
        actual["failures"],
        json!([
            "SecurityError",
            "SecurityError",
            "SecurityError",
            "SecurityError",
            "SecurityError",
            "SecurityError",
            "SecurityError",
            "TypeError"
        ])
    );
    assert_eq!(actual["harmless"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn cross_origin_location_descriptors_keep_the_write_only_surface_and_brands() {
    let source = fixture::Server::start().await;
    let destination = fixture::Server::start().await;
    let mut page = fixture::open(&source.origin, Default::default()).await;
    let child = fixture::child(&mut page).await;
    page.navigate_target(child, &destination.url("/cross"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    let actual = page.eval_json_expression(r#"(()=>{
      const loc=child.location,token=new Error('conversion');
      const failure=fn=>{try{fn();return 'accepted'}catch(error){return error.name}};
      const href=Object.getOwnPropertyDescriptor(loc,'href');
      const replace=Object.getOwnPropertyDescriptor(loc,'replace');
      const then=Object.getOwnPropertyDescriptor(loc,'then');
      const symbols=[Symbol.toStringTag,Symbol.hasInstance,Symbol.isConcatSpreadable];
      const symbolRows=symbols.map(key=>{
        const descriptor=Object.getOwnPropertyDescriptor(loc,key);
        return [loc[key]===undefined,descriptor.value===undefined,
          descriptor.writable,descriptor.enumerable,descriptor.configurable,key in loc];
      });
      let conversion=false;
      try{href.set.call(loc,{toString(){throw token}})}catch(error){conversion=error===token}
      const keys=Reflect.ownKeys(loc);
      return {href:[href.get===undefined,typeof href.set,href.enumerable,href.configurable],
        replace:[replace.value===loc.replace,replace.value===Object.getOwnPropertyDescriptor(loc,'replace').value,
          replace.writable,replace.enumerable,replace.configurable],
        then:[then.value===undefined,then.writable,then.enumerable,then.configurable],
        symbolRows,keys:[keys.includes('href'),keys.includes('replace'),keys.includes('then'),symbols.every(key=>keys.includes(key))],
        failures:[failure(()=>href.set.call({},'/never')),failure(()=>loc.replace.call({},'/never')),
          failure(()=>loc.replace()),failure(()=>loc.replace(Symbol('invalid'))),
          failure(()=>loc.href),failure(()=>Object.getOwnPropertyDescriptor(loc,'origin')),
          failure(()=>('assign' in loc)),failure(()=>{loc.then=1}),
          failure(()=>{loc[Symbol.iterator]=1}),failure(()=>Object.getOwnPropertyDescriptor(loc,Symbol.iterator))],
        conversion};
    })()"#).await.unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(actual["href"], json!([true, "function", false, true]));
    assert_eq!(actual["replace"], json!([true, true, false, false, true]));
    assert_eq!(actual["then"], json!([true, false, false, true]));
    assert_eq!(
        actual["symbolRows"],
        json!(vec![vec![true, true, false, false, true, true]; 3])
    );
    assert_eq!(actual["keys"], json!([true, true, true, true]));
    assert_eq!(
        actual["failures"],
        json!([
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "SecurityError",
            "SecurityError",
            "SecurityError",
            "SecurityError",
            "SecurityError",
            "SecurityError"
        ])
    );
    assert_eq!(actual["conversion"], true);
    assert!(!page.has_pending_auxiliary_work());
    assert_eq!(destination.requests().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_location_components_leave_the_native_url_and_navigation_queue_unchanged() {
    let mut page = PageSession::open(
        "<body>",
        "https://location-contract.test:8010/path?x=1#kept",
        "",
        "",
        5_000,
    )
    .await
    .unwrap();
    let actual = page.eval_json_expression(r#"(()=>{
      const loc=location,before=loc.href;
      const failure=fn=>{try{fn();return 'accepted'}catch(error){return error.name}};
      const invalid=[['protocol','1invalid:'],['hostname','bad host'],['host','['],['port','70000'],['port','not-a-port'],['port','65536']];
      const unchanged=invalid.map(([key,value])=>{loc[key]=value;return loc.href===before});
      const brands=['protocol','host','hostname','port','pathname','search','hash'].map(key=>{
        const descriptor=Object.getOwnPropertyDescriptor(loc,key);
        return [failure(()=>descriptor.get.call({})),failure(()=>descriptor.set.call({},'ignored'))];
      });
      return {unchanged,brands,symbol:failure(()=>{loc.port=Symbol('bad')}),
        readOnly:Reflect.set(loc,'origin','https://forged.test'),href:loc.href};
    })()"#).await.unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(actual["unchanged"], json!(vec![true; 6]));
    assert_eq!(
        actual["brands"],
        json!(vec![vec!["TypeError", "TypeError"]; 7])
    );
    assert_eq!(actual["symbol"], "TypeError");
    assert_eq!(actual["readOnly"], false);
    assert_eq!(
        actual["href"],
        "https://location-contract.test:8010/path?x=1#kept"
    );
    assert!(!page.has_pending_auxiliary_work());
}
