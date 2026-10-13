//! Parsed documents retain actual native ownership in the caller's realm.
use turbo_surf_render::run_with_dom;

#[test]
fn html_parsers_create_inert_independent_documents_and_import_native_nodes() {
    let result=run_with_dom("<head><title>active</title></head><body><main>active</main></body>",r#"(() => {
      const markup='<title>parsed</title><p id="source">original</p><script>globalThis.parserExecuted=true</script><iframe src="https://invalid.test/"></iframe>';
      const docs=[new DOMParser().parseFromString(markup,'text/html'),Document.parseHTMLUnsafe(markup)];
      const checks=docs.map(doc=>{
        const original=doc.querySelector('#source'),copied=document.importNode(original,true);
        document.body.appendChild(copied);copied.textContent='imported';
        const created=doc.createElement('section');doc.body.appendChild(created);doc.title='changed';
        return doc!==document&&doc.defaultView===null&&doc.location===null&&doc.URL===doc.documentURI&&(doc.URL==='about:blank'||doc.URL===document.URL)
          &&doc.baseURI===doc.URL&&doc.readyState==='complete'&&doc.referrer===''&&doc.cookie===''&&doc.contentType==='text/html'
          &&Object.getPrototypeOf(doc)===HTMLDocument.prototype&&doc.constructor===HTMLDocument&&Object.prototype.toString.call(doc)==='[object HTMLDocument]'
          &&doc.head.ownerDocument===doc&&doc.body.ownerDocument===doc&&doc.documentElement.ownerDocument===doc
          &&created.ownerDocument===doc&&original.ownerDocument===doc&&original.textContent==='original'&&copied.ownerDocument===document
          &&doc.querySelector('script').textContent.includes('parserExecuted')&&doc.title==='changed';
      });
      return JSON.stringify({checks,inert:globalThis.parserExecuted===undefined,active:document.title==='active',
        parser:Function.prototype.toString.call(DOMParser).includes('[native code]'),
        methods:[DOMParser.prototype.parseFromString,Document.parseHTMLUnsafe,Document.parseHTML,Document.prototype.importNode].map(fn=>!Object.hasOwn(fn,'prototype')&&Function.prototype.toString.call(fn).includes('[native code]')),
        lengths:[DOMParser.length,DOMParser.prototype.parseFromString.length,Document.parseHTMLUnsafe.length,Document.parseHTML.length,Document.prototype.importNode.length]});
    })()"#).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["checks"], serde_json::json!([true, true]), "{value}");
    assert_eq!(value["lengths"], serde_json::json!([0, 2, 1, 1, 1]));
    for key in ["inert", "active", "parser"] {
        assert_eq!(value[key], true, "{value}");
    }
    assert_eq!(
        value["methods"],
        serde_json::json!([true, true, true, true])
    );
}

#[test]
fn safe_html_parser_filters_native_namespace_trees_and_decoded_script_urls() {
    let result=run_with_dom("<body>active</body>",r#"(() => {
      const safe=Document.parseHTML('<title>safe</title><script>evil()</script><p id="drop" onclick="evil()"><a href="&#x6a;av&#x09;ascript:evil()">link</a></p><iframe></iframe><img src="data:image/png,x"><style>p{color:red}</style><svg onload="evil()"><a href="javascript:evil()">vector</a></svg><math><mi onclick="evil()">x</mi></math><!--drop--><unknown><b>drop</b></unknown><template><script>drop</script></template>');
      return JSON.stringify({head:safe.head.innerHTML,body:safe.body.innerHTML,active:document.body.textContent});
    })()"#).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["head"], "<title>safe</title>");
    assert_eq!(
        value["body"],
        "<p><a>link</a></p><svg><a>vector</a></svg><math><mi>x</mi></math>"
    );
    assert_eq!(value["active"], "active");
}

#[test]
fn parser_receiver_conversion_and_unsupported_modes_fail_explicitly() {
    let result = run_with_dom(
        "<body></body>",
        r#"JSON.stringify([
      ()=>DOMParser.prototype.parseFromString.call({},'<p>x</p>','text/html'),
      ()=>new DOMParser().parseFromString('<p>x</p>'),
      ()=>new DOMParser().parseFromString('<p>x</p>','TEXT/HTML'),
      ()=>new DOMParser().parseFromString(Symbol(),'text/html'),
      ()=>Document.parseHTML(),
      ()=>Document.parseHTMLUnsafe(Symbol()),
      ()=>document.importNode({}),
      ()=>new DOMParser().parseFromString('<x/>','application/xml'),
      ()=>Document.parseHTML('<b>x</b>',{sanitizer:{}}),
      ()=>document.importNode(Document.parseHTMLUnsafe('<p>x</p>'),true),
    ].map(call=>{try{call();return '';}catch(error){return error.name;}}))"#,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value,
        serde_json::json!([
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "NotSupportedError",
            "NotSupportedError",
            "NotSupportedError"
        ])
    );
}
