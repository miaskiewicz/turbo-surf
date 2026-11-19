//! URL conversion uses captured intrinsics while preserving authored coercions.
use serde_json::Value;
use turbo_surf_render::PageSession;

const CONTROLS: &str = r#"(() => {
  const NativeString=String;
  function compare(exercise) {
    function run(poison) {
      let calls=0;
      try {
        if(poison) globalThis.String=function(){calls++;throw Error('ambient String called')};
        const value=exercise();return {calls,value};
      } catch(error) {
        return {calls,error:error.message};
      } finally {globalThis.String=NativeString;}
    }
    const baseline=run(false),poisoned=run(true);
    return {baseline,poisoned,equal:JSON.stringify(baseline)===JSON.stringify(poisoned)};
  }
  const url=compare(() => {
    const conversions=[];
    const input={toString(){conversions.push('input');return 'sub?b=2&a=1'}};
    const base={toString(){conversions.push('base');return 'https://example.test/base/'}};
    const value=new URL(input,base),params=value.searchParams;
    value.pathname='/changed path';value.protocol='http:';value.hostname='other.test';
    value.port='8080';value.username='u';value.password='p';value.hash='tail';value.search='?b=2&a=1';
    params.append('a','3');params.set('b','4');params.sort();
    const mutated=value.href;
    const sentinel={sentinel:true},bad={toString(){throw sentinel}};
    let constructorThrow=false,setterThrow=false;
    try{new URL(bad,'https://example.test/')}catch(error){constructorThrow=error===sentinel}
    try{value.href=bad}catch(error){setterThrow=error===sentinel}
    const preserved=value.href===mutated;
    value.href='https://reset.test/fresh?x=9';
    return {mutated,identity:params===value.searchParams,reset:params.get('x'),
      parsed:URL.parse('/parsed','https://parse.test/root/').href,
      canParse:URL.canParse('child','https://parse.test/root/'),invalid:URL.parse('http://['),
      invalidCanParse:URL.canParse('http://['),constructorThrow,setterThrow,preserved,
      inputConversions:conversions.filter(value=>value==='input').length,
      baseConversions:conversions.filter(value=>value==='base').length};
  });
  const params=compare(() => {
    const trace=[];
    const key={toString(){trace.push('key.toString');return 'key'}};
    const value={ [Symbol.toPrimitive](hint){trace.push('value:'+hint);return 'two words'} };
    const pair=new Proxy([key,value],{get(target,name,receiver){trace.push(typeof name==='symbol'?'pair.iterator':'pair.'+name);return Reflect.get(target,name,receiver)}});
    const sequence=new URLSearchParams([pair]);
    const recordValue={toString(){trace.push('record.toString');return 'record value'}};
    const record=new Proxy({record:recordValue},{
      ownKeys(target){trace.push('record.keys');return Reflect.ownKeys(target)},
      getOwnPropertyDescriptor(target,name){trace.push('record.descriptor:'+name);return Reflect.getOwnPropertyDescriptor(target,name)},
      get(target,name,receiver){trace.push(typeof name==='symbol'?'record.iterator':'record.get:'+name);return Reflect.get(target,name,receiver)}
    });
    const recordParams=new URLSearchParams(record);
    const result=new URLSearchParams('?a=1&a=2&b=3');
    result.append('c','4');result.set('b','5');
    const before=[result.get('a'),result.getAll('a'),result.has('a','2')];
    result.delete('a','1');result.sort();
    const appended={toString(){trace.push('append.toString');return 'extra'}};
    result.append(appended,'value');
    const sentinel={sentinel:true},bad={toString(){throw sentinel}},saved=result.toString();
    let propagated=false;try{result.append(bad,'unreached')}catch(error){propagated=error===sentinel}
    return {sequence:sequence.toString(),record:recordParams.toString(),before,
      serialized:result.toString(),copied:new URLSearchParams(result).toString(),
      propagated,preserved:result.toString()===saved,trace};
  });
  return {url,params};
})()"#;

fn assert_controls(result: Value) {
    assert_eq!(result["url"]["equal"], true, "URL controls: {result:#}");
    assert_eq!(
        result["params"]["equal"], true,
        "parameter controls: {result:#}"
    );
    for section in ["url", "params"] {
        assert_eq!(result[section]["poisoned"]["calls"], 0, "{result:#}");
        assert!(result[section]["poisoned"]["error"].is_null(), "{result:#}");
    }
    let url = &result["url"]["poisoned"]["value"];
    assert_eq!(
        url["mutated"],
        "http://u:p@other.test:8080/changed%20path?a=1&a=3&b=4#tail"
    );
    assert_eq!(url["identity"], true);
    assert_eq!(url["reset"], "9");
    assert_eq!(url["parsed"], "https://parse.test/parsed");
    assert_eq!(url["canParse"], true);
    assert!(url["invalid"].is_null());
    assert_eq!(url["invalidCanParse"], false);
    for flag in ["constructorThrow", "setterThrow", "preserved"] {
        assert_eq!(url[flag], true, "{result:#}");
    }
    assert_eq!(url["inputConversions"], 1);
    assert_eq!(url["baseConversions"], 1);
    let params = &result["params"]["poisoned"]["value"];
    assert_eq!(params["sequence"], "key=two+words");
    assert_eq!(params["record"], "record=record+value");
    assert_eq!(params["before"], serde_json::json!(["1", ["1", "2"], true]));
    assert_eq!(params["serialized"], "a=2&b=5&c=4&extra=value");
    assert_eq!(params["copied"], params["serialized"]);
    assert_eq!(params["propagated"], true);
    assert_eq!(params["preserved"], true);
    let trace = params["trace"].as_array().unwrap();
    for required in [
        "key.toString",
        "value:string",
        "record.toString",
        "append.toString",
    ] {
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry.as_str() == Some(required))
                .count(),
            1,
            "{result:#}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn root_url_and_search_params_ignore_replaced_global_string_but_coerce_authored_values() {
    let mut page = PageSession::open(
        "<body><p>URL controls</p></body>",
        "https://url-controls.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = page.eval_json_expression(CONTROLS).await.unwrap();
    assert_controls(serde_json::from_str(&result).unwrap());
    assert_eq!(
        page.eval_json_expression("String(42)").await.unwrap(),
        "\"42\""
    );
}

#[tokio::test(flavor = "current_thread")]
async fn child_url_and_search_params_capture_their_own_string_intrinsic() {
    let child=format!("<body><p>child URL controls</p><script>globalThis.urlStringResult={CONTROLS};</script></body>");
    let escaped = child.replace('&', "&amp;").replace('"', "&quot;");
    let html = format!("<body><iframe srcdoc=\"{escaped}\"></iframe></body>");
    let mut page = PageSession::open(&html, "https://url-controls.test/", "", "", 5000)
        .await
        .unwrap();
    let result = page
        .eval_json_expression("document.querySelector('iframe').contentWindow.urlStringResult")
        .await
        .unwrap();
    assert_controls(serde_json::from_str(&result).unwrap());
    assert_eq!(
        page.eval_json_expression("document.querySelector('iframe').contentWindow.String(42)")
            .await
            .unwrap(),
        "\"42\""
    );
}
