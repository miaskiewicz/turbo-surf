//! URL component setters (including the ones the URL Standard ignores: an out-of-range
//! port, a special-to-non-special protocol) and TextDecoder fatal / streaming / BOM /
//! legacy-encoding decoding. Expected: Chrome 156 (156.0.8078.4, headless) running the
//! same script on a localhost page.
use turbo_surf_render::PageSession;

const SCRIPT: &str = r#"(()=>{const log=[];const u=new URL('https://a.test:8080/p?q#h');
for(const [k,v] of [['username','us er'],['password','p@ss'],['hostname','b.test'],['host','c.test:9'],['port','70000'],['port','81'],['pathname','/x y'],['search','a=1 2'],['hash','frag ment'],['protocol','http'],['protocol','mailto']]){u[k]=v;log.push(u.href);}
const f=new URL('file:///tmp/x');f.username='u';f.host='h';log.push(f.href);
const d=new TextDecoder('utf-8',{fatal:true});
try{d.decode(new Uint8Array([0xff]));log.push('no')}catch(e){log.push(e.name)}
const s=new TextDecoder('utf-8');log.push(s.decode(new Uint8Array([0xe2,0x82]),{stream:true})+'|'+s.decode(new Uint8Array([0xac])));
const b=new TextDecoder('utf-8',{ignoreBOM:true});log.push(b.decode(new Uint8Array([0xef,0xbb,0xbf,0x41])).length);
const w=new TextDecoder('windows-1252');log.push(w.decode(new Uint8Array([0x80,0xe9])));
const fs=new TextDecoder('utf-8',{fatal:true});try{fs.decode(new Uint8Array([0xe2,0x82]),{stream:true});log.push('pending');fs.decode();log.push('flushed')}catch(e){log.push('flush:'+e.name)}
return log;})()"#;

const CHROME_156: &str = r#"["https://us%20er@a.test:8080/p?q#h","https://us%20er:p%40ss@a.test:8080/p?q#h","https://us%20er:p%40ss@b.test:8080/p?q#h","https://us%20er:p%40ss@c.test:9/p?q#h","https://us%20er:p%40ss@c.test:9/p?q#h","https://us%20er:p%40ss@c.test:81/p?q#h","https://us%20er:p%40ss@c.test:81/x%20y?q#h","https://us%20er:p%40ss@c.test:81/x%20y?a=1%202#h","https://us%20er:p%40ss@c.test:81/x%20y?a=1%202#frag%20ment","http://us%20er:p%40ss@c.test:81/x%20y?a=1%202#frag%20ment","http://us%20er:p%40ss@c.test:81/x%20y?a=1%202#frag%20ment","file://h/tmp/x","TypeError","|€",2,"€é","pending","flush:TypeError"]"#;

#[tokio::test(flavor = "current_thread")]
async fn url_setters_and_text_decoding_match_chrome() {
    let mut page = PageSession::open("<body>", "https://codecs.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(page.eval_json_expression(SCRIPT).await.unwrap(), CHROME_156);
}
