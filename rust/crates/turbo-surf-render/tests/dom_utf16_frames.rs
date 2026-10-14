//! Original DOMString sources cross the native document, script and module seams.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_srcdoc_units_survive_same_origin_and_opaque_frame_execution() {
    if !turbo_surf_render::stable_window_proxy() {
        return;
    }
    let mut page = PageSession::open("<body>", "https://utf16.test/", "", "", 5000)
        .await
        .unwrap();
    let value=page.eval_expression(r#"(async()=>{
      const high=String.fromCharCode(0xd800),low=String.fromCharCode(0xdc00);
      const rows=[];
      for(const opaque of [false,true]) {
        const frame=document.createElement('iframe');
        if(opaque)frame.setAttribute('sandbox','allow-scripts');
        const ready=new Promise((resolve,reject)=>{
          const listener=event=>{if(event.source===frame.contentWindow){removeEventListener('message',listener);resolve(event.data)}};
          addEventListener('message',listener);setTimeout(()=>reject(new Error('original-unit frame timeout')),2000);
        });
        frame.srcdoc='<button onclick="return \''+high+'\';" data-u="'+low+'">'+high+'</button><script>'+"globalThis.exact='"+high+'x'+low+"';function original(){return '"+high+"'};const units=value=>Array.from({length:value.length},(_,i)=>value.charCodeAt(i));const button=document.querySelector('button');parent.postMessage([units(button.getAttribute('data-u')),units(button.textContent),units(exact),units(original()),units(button.onclick.call(button,new Event('click'))),units(original.toString())],'*');"+'</script>';
        document.body.append(frame);rows.push(await ready);frame.remove();
      }
      return JSON.stringify(rows);
    })()"#).await.unwrap();
    let rows: serde_json::Value = serde_json::from_str(&value).unwrap();
    assert_eq!(rows[0], rows[1]);
    for row in rows.as_array().unwrap() {
        assert_eq!(row[0], serde_json::json!([56320]));
        assert_eq!(row[1], serde_json::json!([55296]));
        assert_eq!(row[2], serde_json::json!([55296, 120, 56320]));
        assert_eq!(row[3], serde_json::json!([55296]));
        assert_eq!(row[4], serde_json::json!([55296]));
        let mut expected = "function original(){return '"
            .encode_utf16()
            .collect::<Vec<_>>();
        expected.push(0xd800);
        expected.extend("'}".encode_utf16());
        assert_eq!(row[5], serde_json::json!(expected));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_dynamic_inline_module_retains_original_body_and_source_identity() {
    if !turbo_surf_render::stable_window_proxy() {
        return;
    }
    let mut page = PageSession::open("<body>", "https://utf16.test/document.html", "", "", 5000)
        .await
        .unwrap();
    let probe = include_str!("../../../../harness/detection/dom-utf16-module-probe.js")
        .trim()
        .trim_end_matches(';');
    let value: serde_json::Value = serde_json::from_str(
        &page
            .eval_expression(&format!("(async()=>JSON.stringify(await({probe})))()"))
            .await
            .unwrap(),
    )
    .unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/dom-utf16-module-chrome155.json"
    ))
    .unwrap();
    assert_eq!(value, control["result"]);
}
