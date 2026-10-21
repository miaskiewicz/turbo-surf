//! Guest-generated DOM strings cross the native parser and source compiler intact.
use turbo_surf_render::PageSession;
#[tokio::test(flavor = "current_thread")]
async fn dom_parser_and_reflected_html_preserve_original_lone_units() {
    let mut page = PageSession::open("<body>", "https://utf16.test/", "", "", 5000)
        .await
        .unwrap();
    let value=page.eval_expression(r#"(()=>{const units=value=>Array.from({length:value.length},(_,i)=>value.charCodeAt(i));const high=String.fromCharCode(0xd800),low=String.fromCharCode(0xdc00);const doc=new DOMParser().parseFromString('<p data-u="'+high+'">'+low+'</p>','text/html');const p=doc.querySelector('p'),host=document.createElement('div');host.innerHTML='<p data-u="'+low+'">'+high+'</p>';document.body.append(host);const imported=document.importNode(p,true);return JSON.stringify([units(p.textContent),units(p.getAttribute('data-u')),units(host.firstChild.textContent),units(host.firstChild.getAttribute('data-u')),units(imported.textContent),units(imported.getAttribute('data-u'))]);})()"#).await.unwrap();
    assert_eq!(value, "[[56320],[55296],[55296],[56320],[56320],[55296]]");
}
#[tokio::test(flavor = "current_thread")]
async fn native_dynamic_inline_script_compiles_original_source_units() {
    let mut page = PageSession::open("<body>", "https://utf16.test/", "", "", 5000)
        .await
        .unwrap();
    let value=page.eval_expression(r#"(async()=>{const high=String.fromCharCode(0xd800),low=String.fromCharCode(0xdc00),script=document.createElement('script');script.textContent="globalThis.__utf16Source='"+high+"x"+low+"';";document.body.append(script);await new Promise(resolve=>setTimeout(resolve,0));return JSON.stringify(Array.from({length:__utf16Source.length},(_,i)=>__utf16Source.charCodeAt(i)));})()"#).await.unwrap();
    assert_eq!(value, "[55296,120,56320]");
}
