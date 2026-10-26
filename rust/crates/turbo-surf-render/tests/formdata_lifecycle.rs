use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};
use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;
fn server(responses: Vec<String>) -> (String, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            tx.send(String::from_utf8(bytes).unwrap()).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (base, rx)
}
fn response(status: &str, headers: &str, body: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",body.len())
}
fn request(rx: &Receiver<String>) -> String {
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn actual_formdata_interface_and_constructor_reentrancy_match_chrome() {
    for (probe, raw) in [
        (
            test_support::FORMDATA_EVENT_PROBE_JS,
            test_support::FORMDATA_EVENT_CHROME155_JSON,
        ),
        (
            test_support::FORMDATA_GUARDS_PROBE_JS,
            test_support::FORMDATA_GUARDS_CHROME155_JSON,
        ),
    ] {
        let control: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(control["browser"], "Chrome/155.0.8059.80");
        assert_eq!(
            control["probeSha256"],
            format!("{:x}", Sha256::digest(probe.as_bytes()))
        );
        assert_eq!(control["errors"], serde_json::json!([]));
        let mut page = PageSession::open(
            "<!doctype html><body>",
            "https://example.test/",
            "",
            "",
            5000,
        )
        .await
        .unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &page
                .eval_json_expression(probe.trim().trim_end_matches(';'))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(actual, control["result"][0]["result"]);
        page.close();
    }
}
#[tokio::test(flavor = "current_thread")]
async fn formdata_listener_changes_real_wire_entries_but_not_accepted_metadata() {
    let raw = include_str!(
        "../../../../harness/detection/fixtures/formdata-lifecycle-wire-chrome155.json"
    );
    let control: serde_json::Value = serde_json::from_str(raw).unwrap();
    let probe = include_str!("../../../../harness/detection/formdata-lifecycle-wire-probe.js");
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(probe.as_bytes()))
    );
    for expected in control["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| !row["wire"].is_null())
    {
        let mode = expected["mode"].as_str().unwrap();
        let (base, rx) = server(vec![response("200 OK", "", "<p>actual response</p>")]);
        let mut page = PageSession::open(
            "<form action='/wire?before=1#before'><input name='initial' value='one'></form>",
            &format!("{base}/start"),
            "",
            "",
            5000,
        )
        .await
        .unwrap();
        let script = format!(
            r#"(()=>{{
   const mode={mode},form=document.querySelector('form'),events=[];
   form.addEventListener('submit',()=>events.push('submit'));
   form.addEventListener('formdata',event=>{{
    events.push({{type:event.type,trusted:event.isTrusted,bubbles:event.bubbles,cancelable:event.cancelable,entries:[...event.formData]}});
    event.formData.set('initial','changed');event.formData.append('event','two');
    if(mode==='all-metadata'||mode==='action')form.action='/after?after=1#after';
    if(mode==='all-metadata'||mode==='method')form.method='post';
    if(mode==='all-metadata'||mode==='enctype')form.enctype='text/plain';
    if(mode==='all-metadata'||mode==='target')form.target='_blank';
    if(mode==='prevent-default')event.preventDefault();
   }});
   if(mode==='submit-bypass')form.submit();else form.requestSubmit();
   return events;
  }})()"#,
            mode = serde_json::to_string(mode).unwrap()
        );
        let actual: serde_json::Value =
            serde_json::from_str(&page.eval_json_expression(&script).await.unwrap()).unwrap();
        assert_eq!(actual, expected["events"], "{mode}");
        let wire = request(&rx);
        assert!(
            wire.starts_with(&format!(
                "GET {} HTTP/1.1\r\n",
                expected["wire"]["url"].as_str().unwrap()
            )),
            "{mode}: {wire}"
        );
        assert_eq!(
            page.navigation_snapshot().1,
            format!("{base}{}#before", expected["wire"]["url"].as_str().unwrap())
        );
        assert!(page.serialize().contains("actual response"));
        page.close();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_cancelled_and_csp_blocked_formdata_order_matches_real_chrome() {
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/formdata-lifecycle-wire-chrome155.json"
    ))
    .unwrap();
    for expected in control["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["wire"].is_null())
    {
        let mode = expected["mode"].as_str().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let mut page=PageSession::open("<!doctype html><head></head><body><form action='/wire'><input name='initial' value='one'></form>",&format!("{base}/start"),"","",5000).await.unwrap();
        let script = format!(
            r#"(()=>{{
   const mode={mode},form=document.querySelector('form'),input=form.firstChild,events=[];globalThis.events=events;
   input.addEventListener('invalid',()=>events.push('invalid'));
   if(mode==='invalid'){{input.required=true;input.value='';}}
   form.addEventListener('submit',event=>{{events.push('submit');if(mode==='cancel-submit')event.preventDefault();}});
   form.addEventListener('formdata',event=>{{events.push({{type:event.type,trusted:event.isTrusted,bubbles:event.bubbles,cancelable:event.cancelable,entries:[...event.formData]}});event.formData.set('initial','changed');event.formData.append('event','two');}});
   if(mode==='csp-blocked'){{document.addEventListener('securitypolicyviolation',event=>events.push({{policy:event.effectiveDirective,disposition:event.disposition}}));const meta=document.createElement('meta');meta.httpEquiv='Content-Security-Policy';meta.content="form-action 'none'";document.head.append(meta);}}
   form.requestSubmit();
  }})()"#,
            mode = serde_json::to_string(mode).unwrap()
        );
        page.eval(&script).await.unwrap();
        let actual: serde_json::Value =
            serde_json::from_str(&page.eval_json_expression("events").await.unwrap()).unwrap();
        assert_eq!(actual, expected["events"], "{mode}");
        assert_eq!(page.navigation_snapshot().0, 0);
        assert!(
            matches!(listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock),
            "{mode}: unexpected HTTP request"
        );
        page.close();
    }
}
