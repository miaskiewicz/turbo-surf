//! URL owns canonical parameters even when the public constructor is replaced.
use serde_json::Value;
use turbo_surf_render::PageSession;

const CONTROLS: &str = r#"(() => {
  const Original=URLSearchParams, descriptor=Object.getOwnPropertyDescriptor(globalThis,'URLSearchParams');
  const observations=[];
  for(const mode of ['throwing','proxy','getter']) {
    let reads=0,calls=0,constructs=0;
    const marker={mode};
    function GuestConstructor(){calls++;throw marker}
    const replacement=mode==='proxy'?new Proxy(GuestConstructor,{
      get(target,key,receiver){reads++;return Reflect.get(target,key,receiver)},
      construct(){constructs++;throw marker}
    }):GuestConstructor;
    if(mode==='getter') Object.defineProperty(globalThis,'URLSearchParams',{
      configurable:true,get(){reads++;return replacement}
    });
    else globalThis.URLSearchParams=replacement;
    const row={mode};
    try {
      const coercions=[];
      const input={toString(){coercions.push('input');return 'start?x=1'}};
      const base={toString(){coercions.push('base');return 'https://example.test/base/'}};
      const value=new URL(input,base),params=value.searchParams;
      const initial=value.href;
      value.search={toString(){coercions.push('search');return '?next=2'}};
      const searched=value.href;
      value.pathname={ [Symbol.toPrimitive](hint){coercions.push('path:'+hint);return '/moved'} };
      const moved=value.href;
      value.href={toString(){coercions.push('href');return 'https://other.test/fresh?z=9'}};
      params.append('extra',{toString(){coercions.push('append');return '10'}});
      const final=value.href;
      const ownMarker={},bad={toString(){throw ownMarker}};
      let setterThrow=false,constructorThrow=false;
      try{value.search=bad}catch(error){setterThrow=error===ownMarker}
      try{new URL(bad,'https://example.test/')}catch(error){constructorThrow=error===ownMarker}
      const parsed=URL.parse('/parsed?y=7','https://parse.test/base/');
      row.value={initial,searched,moved,final,identity:value.searchParams===params,
        canonical:params instanceof Original,prototype:Object.getPrototypeOf(params)===Original.prototype,
        parsed:parsed.href,parsedCanonical:parsed.searchParams instanceof Original,
        parsedValue:parsed.searchParams.get('y'),canParse:URL.canParse('child','https://parse.test/base/'),
        setterThrow,constructorThrow,preserved:value.href===final,
        inputCalls:coercions.filter(name=>name==='input').length,
        baseCalls:coercions.filter(name=>name==='base').length,
        searchCalls:coercions.filter(name=>name==='search').length,
        pathCalls:coercions.filter(name=>name==='path:string').length,
        hrefCalls:coercions.filter(name=>name==='href').length,
        appendCalls:coercions.filter(name=>name==='append').length};
    } catch(error) {row.error=error===marker?'guest constructor called':'unexpected URL error';}
    finally {
      row.internal={reads,calls,constructs};
      const previous={reads,calls,constructs};
      try{new globalThis.URLSearchParams('explicit app call')}catch(error){row.publicMarker=error===marker}
      row.publicDelta={reads:reads-previous.reads,calls:calls-previous.calls,constructs:constructs-previous.constructs};
      Object.defineProperty(globalThis,'URLSearchParams',descriptor);
    }
    observations.push(row);
  }
  return observations;
})()"#;

fn assert_controls(result: Value) {
    let rows = result.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert!(row["error"].is_null(), "{row:#}");
        assert_eq!(
            row["internal"],
            serde_json::json!({"reads":0,"calls":0,"constructs":0}),
            "{row:#}"
        );
        assert_eq!(
            row["publicMarker"], true,
            "the public replacement must stay callable: {row:#}"
        );
        let mode = row["mode"].as_str().unwrap();
        let expected = match mode {
            "throwing" => serde_json::json!({"reads":0,"calls":1,"constructs":0}),
            "proxy" => serde_json::json!({"reads":0,"calls":0,"constructs":1}),
            "getter" => serde_json::json!({"reads":1,"calls":1,"constructs":0}),
            _ => unreachable!(),
        };
        assert_eq!(row["publicDelta"], expected, "{row:#}");
        let value = &row["value"];
        assert_eq!(value["initial"], "https://example.test/base/start?x=1");
        assert_eq!(value["searched"], "https://example.test/base/start?next=2");
        assert_eq!(value["moved"], "https://example.test/moved?next=2");
        assert_eq!(value["final"], "https://other.test/fresh?z=9&extra=10");
        assert_eq!(value["parsed"], "https://parse.test/parsed?y=7");
        assert_eq!(value["parsedValue"], "7");
        for flag in [
            "identity",
            "canonical",
            "prototype",
            "parsedCanonical",
            "canParse",
            "setterThrow",
            "constructorThrow",
            "preserved",
        ] {
            assert_eq!(value[flag], true, "{row:#}");
        }
        for count in [
            "inputCalls",
            "baseCalls",
            "searchCalls",
            "pathCalls",
            "hrefCalls",
            "appendCalls",
        ] {
            assert_eq!(value[count], 1, "{row:#}");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn root_url_retains_canonical_params_without_freezing_the_public_constructor() {
    let mut page = PageSession::open(
        "<body><p>constructor controls</p></body>",
        "https://url-constructor.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = page.eval_json_expression(CONTROLS).await.unwrap();
    assert_controls(serde_json::from_str(&result).unwrap());
    assert_eq!(
        page.eval_json_expression("new URLSearchParams('healthy=1').get('healthy')")
            .await
            .unwrap(),
        "\"1\""
    );
}

#[tokio::test(flavor = "current_thread")]
async fn child_url_retains_its_canonical_params_without_reading_the_public_constructor() {
    let child =
        format!("<body><script>globalThis.paramsConstructorResult={CONTROLS};</script></body>");
    let escaped = child.replace('&', "&amp;").replace('"', "&quot;");
    let html = format!("<body><iframe srcdoc=\"{escaped}\"></iframe></body>");
    let mut page = PageSession::open(&html, "https://url-constructor.test/", "", "", 5000)
        .await
        .unwrap();
    let result = page
        .eval_json_expression(
            "document.querySelector('iframe').contentWindow.paramsConstructorResult",
        )
        .await
        .unwrap();
    assert_controls(serde_json::from_str(&result).unwrap());
    assert_eq!(page.eval_json_expression("new (document.querySelector('iframe').contentWindow.URLSearchParams)('healthy=1').get('healthy')").await.unwrap(),"\"1\"");
}
