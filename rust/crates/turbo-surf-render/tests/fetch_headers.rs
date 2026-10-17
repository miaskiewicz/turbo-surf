//! Fetch header guards enforce browser-controlled request and response state.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::run_with_dom;

#[test]
fn request_guards_drop_forbidden_and_non_safelisted_headers_on_creation_and_mutation() {
    let value=run_with_dom("<body></body>",r#"(()=>{
      const request=new Request('https://target.test/',{headers:{Cookie:'secret',Origin:'https://spoof.test','Sec-Fetch-Site':'none','X-Token':'value'}});
      request.headers.set('Cookie','override');request.headers.append('X-Token','second');
      const opaque=new Request('https://target.test/',{mode:'no-cors',headers:{'X-Token':'value','Content-Type':'application/json',Accept:'text/html','Accept-Language':'en-US'}});
      opaque.headers.set('Content-Type','text/plain');opaque.headers.set('X-Token','later');opaque.headers.append('Content-Type','application/json');
      const invalid=callback=>{try{callback();return false}catch(error){return error instanceof TypeError}};
      return JSON.stringify({request:Array.from(request.headers),opaque:Array.from(opaque.headers),invalid:[invalid(()=>new Request('https://target.test/',{mode:'no-cors',method:'PUT'})),invalid(()=>new Headers([[Symbol(),'value']])),invalid(()=>new Headers([['key',Symbol()]]))]});
    })()"#).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&value).unwrap(),
        serde_json::json!({"request":[["x-token","value, second"]],"opaque":[["accept","text/html"],["accept-language","en-US"],["content-type","text/plain"]],"invalid":[true,true,true]})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn opaque_bodies_and_response_headers_stay_private_when_guest_intrinsics_change() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}/resource", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0; 4096];
                let _ = socket.read(&mut buffer).await;
                let response="HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nAccess-Control-Allow-Origin: *\r\nX-Secret: hidden-header\r\nSet-Cookie: secret=hidden-cookie; HttpOnly\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhidden-body";
                socket.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    let mut page =
        turbo_surf_render::PageSession::open("<body></body>", "https://source.test/", "", "", 5000)
            .await
            .unwrap();
    let script = format!(
        r#"const originalJSON=JSON.stringify;
      JSON.stringify=()=>originalJSON({{source:'{target}',mode:'same-origin'}});
      Object.defineProperty(URL.prototype,'origin',{{get(){{return 'https://source.test'}}}});
      globalThis[Symbol.for('ts')].responseFromNetwork=()=>{{throw new Error('public hook')}};
      (async()=>{{
        const opaque=await fetch('{target}',{{mode:'no-cors'}});
        const opaqueBody=await opaque.text();
        const cors=await fetch('{target}');
        globalThis.__RESULT=originalJSON({{opaque:[opaque.type,opaque.status,opaqueBody,Array.from(opaque.headers)],cors:[cors.type,cors.headers.get('x-secret'),cors.headers.get('set-cookie'),await cors.text()],publicLog:globalThis[Symbol.for('ts')].netLog!==undefined}});
      }})();"#
    );
    let value: serde_json::Value =
        serde_json::from_str(&page.eval(&script).await.unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"opaque":["opaque",0,"",[]],"cors":["cors",null,null,"hidden-body"],"publicLog":false})
    );
    let records: serde_json::Value = serde_json::from_str(&page.drain_network_log()).unwrap();
    assert_eq!(records.as_array().unwrap().len(), 2);
    assert!(records
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["body"] == "hidden-body"));
    assert_eq!(page.drain_network_log(), "[]");
    server.abort();
}
