use super::*;

fn run(html: &str, code: &str) -> serde_json::Value {
    super::tests::init_v8();
    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope,isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    install_with_html(scope, html);
    let global = scope.get_current_context().global(scope);
    let key = v8::String::new(scope, "document").unwrap();
    let document = global
        .get(scope, key.into())
        .unwrap()
        .to_object(scope)
        .unwrap();
    let record = get_interface_record(scope, document)
        .unwrap()
        .to_object(scope)
        .unwrap();
    let key = v8::String::new(scope, "cssHost").unwrap();
    let host = record.get(scope, key.into()).unwrap();
    let key = v8::String::new(scope, "__cssTestHost").unwrap();
    global.set(scope, key.into(), host);
    let code = v8::String::new(scope, code).unwrap();
    v8::tc_scope!(let scope,scope);
    let value = v8::Script::compile(scope, code, None)
        .unwrap_or_else(|| panic!("{}", scope.exception().unwrap().to_rust_string_lossy(scope)))
        .run(scope)
        .unwrap_or_else(|| panic!("{}", scope.exception().unwrap().to_rust_string_lossy(scope)));
    let value = value.to_rust_string_lossy(scope);
    reset();
    serde_json::from_str(&value).unwrap()
}

#[test]
fn native_style_text_ignores_own_getters_and_preserves_sheet_identity_until_actual_mutation() {
    let value = run(
        "<style id=s>.real {color:red}</style>",
        r#"(()=>{
      const style=document.getElementById('s');let reads=0;
      Object.defineProperty(style,'textContent',{configurable:true,get(){reads++;return '.fake {color:blue}';}});
      const original=style.sheet;
      original.insertRule('.inserted {width:20px}',1);
      const retained=style.sheet===original;
      const first=original.cssRules[0].selectorText;
      const setter=Object.getOwnPropertyDescriptor(Node.prototype,'textContent').set;
      setter.call(style,'.updated {height:30px}');
      const changed=style.sheet!==original;
      return JSON.stringify({reads,retained,first,changed,current:style.sheet.cssRules[0].selectorText,oldCount:original.cssRules.length});
    })()"#,
    );
    assert_eq!(
        value,
        serde_json::json!({"reads":0,"retained":true,"first":".real","changed":true,"current":".updated","oldCount":2})
    );
}

#[test]
fn private_css_export_reads_real_mutations_disabled_and_media_without_public_hooks() {
    let value = run(
        "<style id=s media=screen>.real {color:red}</style>",
        r#"(()=>{
      const style=document.getElementById('s'),sheet=style.sheet,rule=sheet.cssRules[0];
      rule.style.setProperty('width','27px','important');sheet.disabled=true;
      let reads=0;
      for(const [object,key] of [[style,'textContent'],[style,'getAttribute'],[sheet,'cssRules'],[sheet,'disabled'],[sheet,'media'],[rule,'cssText'],[rule,'style']])Object.defineProperty(object,key,{configurable:true,get(){reads++;throw Error('public hook');}});
      const savedMap=Map.prototype.get,savedArray=Array.prototype.map,savedIterator=Array.prototype[Symbol.iterator];
      Map.prototype.get=Array.prototype.map=Array.prototype[Symbol.iterator]=function(){reads++;throw Error('prototype hook');};
      Object.defineProperty(Object.prototype,'cssRules',{configurable:true,get(){reads++;throw Error('inherited public hook');}});
      let exported;try{exported=__cssTestHost.exportSheet(sheet);}finally{delete Object.prototype.cssRules;Map.prototype.get=savedMap;Array.prototype.map=savedArray;Array.prototype[Symbol.iterator]=savedIterator;}
      return JSON.stringify({reads,exported,nullArrays:Object.getPrototypeOf(exported.rules)===null,nullRecord:Object.getPrototypeOf(exported)===null});
    })()"#,
    );
    assert_eq!(value["reads"], 0);
    assert_eq!(value["nullArrays"], true);
    assert_eq!(value["nullRecord"], true);
    assert_eq!(value["exported"]["disabled"], true);
    assert_eq!(value["exported"]["media"], "screen");
    assert_eq!(
        value["exported"]["rules"][0]["cssText"],
        ".real { color: red; width: 27px !important; }"
    );
}

#[test]
fn private_css_export_retains_group_changes_import_ownership_and_cross_origin_policy() {
    let value = run("<style id=s>@import url('child.css') screen; @media screen { .inside {color:red} }</style>",r#"(()=>{
      const parent=document.getElementById('s').sheet,imported=parent.cssRules[0],group=parent.cssRules[1];
      group.cssRules[0].style.color='blue';
      const child=new CSSStyleSheet();__cssTestHost.attachImport(imported,child,'https://other.test/child.css','.child {width:33px}',false);
      let denied='';try{void child.cssRules;}catch(error){denied=error.name;}
      let hooks=0;Object.defineProperty(group,'cssRules',{get(){hooks++;throw Error('public grouping hook');}});
      const exported=__cssTestHost.exportSheet(parent);
      return JSON.stringify({hooks,denied,exported});
    })()"#);
    assert_eq!(value["hooks"], 0);
    assert_eq!(value["denied"], "SecurityError");
    let rules = &value["exported"]["rules"];
    assert_eq!(rules[0]["styleSheet"]["originClean"], false);
    assert_eq!(
        rules[0]["styleSheet"]["href"],
        "https://other.test/child.css"
    );
    assert_eq!(
        rules[0]["styleSheet"]["rules"][0]["cssText"],
        ".child { width: 33px; }"
    );
    assert_eq!(rules[1]["prelude"], "@media screen");
    assert_eq!(rules[1]["rules"][0]["cssText"], ".inside { color: blue; }");
    assert_eq!(
        rules[1]["cssText"],
        "@media screen { .inside { color: blue; } }"
    );
}

#[test]
fn private_css_export_observes_native_media_attributes_and_bounded_cycles() {
    let value = run(
        "<style id=s media=print>@import url('child.css'); .root {height:8px}</style>",
        r#"(()=>{
      const style=document.getElementById('s'),sheet=style.sheet;
      style.setAttribute('media','screen and (min-width: 10px)');
      let hooks=0;Object.defineProperty(style,'getAttribute',{value(){hooks++;throw Error('public attribute hook');}});
      __cssTestHost.attachImport(sheet.cssRules[0],sheet,'https://layout.test/root.css',undefined,true);
      const exported=__cssTestHost.exportSheet(sheet);
      let forged='';try{__cssTestHost.exportSheet({});}catch(error){forged=error.name;}
      return JSON.stringify({hooks,forged,exported});
    })()"#,
    );
    assert_eq!(value["hooks"], 0);
    assert_eq!(value["forged"], "TypeError");
    assert_eq!(value["exported"]["media"], "screen and (min-width: 10px)");
    assert_eq!(value["exported"]["rules"][0]["styleSheet"]["cycle"], true);
}

#[test]
fn private_css_export_retains_font_face_declaration_changes() {
    let value = run(
        "<style id=s>@font-face { font-family: test; src: url(font.woff2); }</style>",
        r#"(()=>{
      const sheet=document.getElementById('s').sheet,rule=sheet.cssRules[0];
      rule.style.setProperty('font-family','updated');
      let reads=0;Object.defineProperty(rule,'style',{get(){reads++;throw Error('public font hook');}});
      const exported=__cssTestHost.exportSheet(sheet);
      return JSON.stringify({reads,text:exported.rules[0].cssText});
    })()"#,
    );
    assert_eq!(value["reads"], 0);
    assert_eq!(
        value["text"],
        "@font-face { font-family: updated; src: url(\"font.woff2\"); }"
    );
}

#[test]
fn public_live_css_behavior_matches_independent_chrome_control() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("css_live_chrome155.json")).unwrap();
    let probe = include_str!("css_live_probe.js")
        .trim()
        .trim_end_matches(';');
    assert_eq!(
        run(
            "<!doctype html><html><head></head><body></body></html>",
            &format!("JSON.stringify({probe})")
        ),
        expected["result"]
    );
}

#[test]
fn private_css_export_enforces_its_actual_serialized_text_budget() {
    let value = run(
        "<style id=s>@media screen { .inside {color:red} }</style>",
        r#"(()=>{
      const sheet=document.getElementById('s').sheet;
      sheet.cssRules[0].cssRules[0].style.setProperty('--large','x'.repeat(1100000));
      let failure='';try{__cssTestHost.exportSheet(sheet);}catch(error){failure=error.name+':'+error.message;}
      return JSON.stringify(failure);
    })()"#,
    );
    assert_eq!(value, "RangeError:Live stylesheet text budget exceeded");
}
