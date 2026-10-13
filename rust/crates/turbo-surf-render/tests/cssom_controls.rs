//! Offline browser-contract checks for stylesheet text and parsed form controls.
use turbo_surf_render::PageSession;

async fn evaluate(html: &str, js: &str) -> serde_json::Value {
    let mut page = PageSession::open(html, "https://example.test/", "", "", 5000)
        .await
        .unwrap();
    let result = page.eval(js).await.unwrap();
    page.close();
    serde_json::from_str(&result).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn parsed_and_inserted_controls_expose_values_and_reflected_properties() {
    let result = evaluate(
        r#"<body><form id="f"><input id="token" type="hidden" name="ei" value="abc&amp;def"><textarea id="q" name="q">search text</textarea></form></body>"#,
        r#"
        const token=document.querySelector('#token'),q=document.querySelector('#q');
        const initial={value:token.value,name:token.name,type:token.type,form:token.form.id,text:q.value};
        document.body.insertAdjacentHTML('beforeend','<input id="late" type="hidden" value="late token">');
        token.value='updated';
        globalThis.__RESULT=JSON.stringify({initial,updated:token.value,late:document.querySelector('#late').value});
        "#,
    ).await;
    assert_eq!(result["initial"]["value"], "abc&def");
    assert_eq!(result["initial"]["name"], "ei");
    assert_eq!(result["initial"]["type"], "hidden");
    assert_eq!(result["initial"]["form"], "f");
    assert_eq!(result["initial"]["text"], "search text");
    assert_eq!(result["updated"], "updated");
    assert_eq!(result["late"], "late token");
}

#[tokio::test(flavor = "current_thread")]
async fn stylesheet_text_parses_and_mutations_keep_cssom_separate_from_dom() {
    let result = evaluate(
        "<body><style>body { color: red; } sentinel {}</style></body>",
        r#"
        const style=document.querySelector('style'),sheet=style.sheet;
        const initial={count:sheet.cssRules.length,selector:sheet.cssRules[1].selectorText,rule:sheet.cssRules[1] instanceof CSSStyleRule,color:sheet.cssRules[0].style.getPropertyValue('color'),registered:document.styleSheets[0]===sheet};
        const text=style.textContent;
        sheet.insertRule('a { color: blue; }',1);
        const inserted=sheet.cssRules.length===3 && sheet.cssRules[1].selectorText==='a' && style.textContent===text;
        sheet.deleteRule(1);
        style.textContent='/* comment */ h1 { content: "}"; } @media screen { p { color: green; } }';
        const replacement=style.sheet;
        const updated=replacement!==sheet && sheet.cssRules.length===2 && sheet.ownerNode===null && replacement.cssRules.length===2 && replacement.cssRules[0].style.getPropertyValue('content')==='"}"' && replacement.cssRules[1].cssRules[0].selectorText==='p';
        style.remove();
        globalThis.__RESULT=JSON.stringify({initial,inserted,updated,removed:document.styleSheets.length===0 && style.sheet===null});
        "#,
    ).await;
    assert_eq!(result["initial"]["count"], 2);
    assert_eq!(result["initial"]["selector"], "sentinel");
    assert_eq!(result["initial"]["rule"], true);
    assert_eq!(result["initial"]["color"], "red");
    assert_eq!(result["initial"]["registered"], true);
    for field in ["inserted", "updated", "removed"] {
        assert_eq!(result[field], true, "{result}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn constructed_sheets_parse_rule_lists_and_validate_insertion_indexes() {
    let result = evaluate(
        "<body></body>",
        r#"
        const sheet=new CSSStyleSheet();
        sheet.replaceSync('a { color: red; } b {}');
        const defaultIndex=sheet.insertRule('first {}');
        const selectors=Array.from(sheet.cssRules,r=>r.selectorText);
        let bounds='',syntax='';
        try {sheet.insertRule('last {}',99);}catch(e){bounds=e.name;}
        try {sheet.insertRule('a {} b {}',0);}catch(e){syntax=e.name;}
        globalThis.__RESULT=JSON.stringify({defaultIndex,selectors,bounds,syntax,owner:sheet.ownerNode});
        "#,
    ).await;
    assert_eq!(result["defaultIndex"], 0);
    assert_eq!(result["selectors"], serde_json::json!(["first", "a", "b"]));
    assert_eq!(result["bounds"], "IndexSizeError");
    assert_eq!(result["syntax"], "SyntaxError");
    assert_eq!(result["owner"], serde_json::Value::Null);
}

#[tokio::test(flavor = "current_thread")]
async fn cloned_parsed_controls_preserve_token_attributes_and_interface_getters() {
    let result = evaluate(
        r#"<body><form><input name="ei" type="hidden" value="one"><input name="ei" type="hidden" value="two"><textarea>initial text</textarea><select><option value="first">First</option><option selected value="second">Second</option></select></form></body>"#,
        r#"
        const form=document.querySelector('form');
        const first=form.querySelector('input');first.value='updated';
        const clone=form.cloneNode(true);document.body.appendChild(clone);
        const tokens=Array.from(clone.querySelectorAll('input'),e=>({value:e.value,attribute:e.getAttribute('value'),name:e.name,type:e.type}));
        const select=clone.querySelector('select');
        globalThis.__RESULT=JSON.stringify({tokens,text:clone.querySelector('textarea').value,selected:select.value,index:select.selectedIndex});
        "#,
    ).await;
    assert_eq!(
        result["tokens"],
        serde_json::json!([
            {"value":"updated","attribute":"updated","name":"ei","type":"hidden"},
            {"value":"two","attribute":"two","name":"ei","type":"hidden"}
        ])
    );
    assert_eq!(result["text"], "initial text");
    assert_eq!(result["selected"], "second");
    assert_eq!(result["index"], 1);
}

#[tokio::test(flavor = "current_thread")]
async fn live_control_values_preserve_defaults_and_explicit_empty_selection() {
    let result = evaluate(
        r#"<body><input value="initial"><textarea value="ignored">initial text</textarea><select><option disabled value="d">D</option><option value="a">A</option><option value="b">B</option></select></body>"#,
        r#"
        const input=document.querySelector('input'), text=document.querySelector('textarea'), select=document.querySelector('select');
        const before=[input.value,text.value,select.value,select.selectedIndex];
        input.value='edited';input.defaultValue='new default';
        text.value='live';text.defaultValue='default';select.value='missing';
        const after=[input.value,input.getAttribute('value'),text.value,text.textContent,select.value,select.selectedIndex];
        const clones=[input.cloneNode(true).value,text.cloneNode(true).value,select.cloneNode(true).selectedIndex];
        globalThis.__RESULT=JSON.stringify({before,after,clones});
        "#,
    ).await;
    assert_eq!(
        result["before"],
        serde_json::json!(["initial", "initial text", "a", 1])
    );
    assert_eq!(
        result["after"],
        serde_json::json!(["edited", "new default", "live", "default", "", -1])
    );
    assert_eq!(result["clones"], serde_json::json!(["edited", "live", -1]));
}
