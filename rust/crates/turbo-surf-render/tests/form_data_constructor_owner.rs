use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const HTML: &str = "<!doctype html><form id=form><input name=field value=value></form>";
const READ: &str = r#"(async()=>{
 const form=document.getElementById('form');let events=0;
 form.addEventListener('formdata',event=>{events++;event.formData.append('event','seen');},{once:true});
 const data=new FormData();data.append('text','value');
 const file=new File(['payload'],'historical.csv',{type:'text/csv'});data.append('file',file);
 const fromForm=Array.from(new FormData(form));
 const decoded=await new Request(location.href,{method:'POST',body:data}).formData();
 return JSON.stringify({text:data.get('text'),sameFile:data.get('file')===file,
  name:decoded.get('file').name,type:decoded.get('file').type,body:await decoded.get('file').text(),
  fromForm,events});
})()"#;

async fn serve(listener: tokio::net::TcpListener) {
    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).await.unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&bytes[..count]);
            assert!(request.len() <= 8192);
        }
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", HTML.len(), HTML);
        let _ = socket.write_all(response.as_bytes()).await;
    }
}

fn collect(page: &mut PageSession) {
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
}

async fn check_entries(page: &mut PageSession) {
    let value = page.eval_expression(READ).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&value).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"text":"value","sameFile":true,"name":"historical.csv","type":"text/csv","body":"payload","fromForm":[["field","value"],["event","seen"]],"events":1})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn constructor_keeps_its_document_after_navigation_collection_and_borrowing() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    std::env::set_var(
        "TURBO_SURF_ALLOWED_ORIGINS",
        serde_json::to_string(&[&origin]).unwrap(),
    );
    let server = tokio::spawn(serve(listener));
    let mut page = PageSession::open(HTML, &origin, "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    for document in 0..3 {
        if document > 0 {
            page.navigate_target(root, &format!("{origin}/document-{document}"), "replace")
                .await
                .unwrap();
        }
        check_entries(&mut page).await;
        collect(&mut page);
        check_entries(&mut page).await;
    }
    page.eval_expression("globalThis.holder=window.open('about:blank');holder.savedConstructor=FormData;holder.savedForm=document.getElementById('form');'held'").await.unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|p| p.target != root.to_string())
        .unwrap()
        .target
        .parse()
        .unwrap();
    page.navigate_target(root, &format!("{origin}/replacement"), "replace")
        .await
        .unwrap();
    collect(&mut page);
    assert_eq!(page.eval_target_expression(peer, "(()=>{const data=new savedConstructor();data.append('held','yes');return JSON.stringify([data.get('held'),Array.from(new savedConstructor(savedForm)),data instanceof savedConstructor]);})()", false, false).await.unwrap(), r#"["yes",[["field","value"]],true]"#);
    check_entries(&mut page).await;
    page.close();
    server.abort();
}
