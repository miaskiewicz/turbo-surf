use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};
use turbo_surf_render::PageSession;

fn chrome_fixture(raw: &str, probe: &str) -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(
        value["probeSha256"],
        format!("{:x}", Sha256::digest(probe.as_bytes()))
    );
    assert_eq!(value["browser"], "Chrome/155.0.8059.80");
    assert_eq!(value["v8"], "15.5.35.23");
    value
}

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
async fn get_submission_fetches_and_installs_actual_response_document() {
    let (base, rx) = server(vec![response(
        "201 Created",
        "Set-Cookie: private=yes; HttpOnly; Path=/\r\n",
        "<script>globalThis.loaded='response'</script><p id='answer'>ok</p>",
    )]);
    let html="<base href='/base/'><form action='search?old=1#kept'><input name='q' value='hello world'><input name='dup' value='one'><input name='dup' value='two'></form><script>globalThis.oldGlobal=true</script>";
    let mut page = PageSession::open(html, &format!("{base}/start"), "", "", 5000)
        .await
        .unwrap();
    page.eval("document.cookie='before=yes; Path=/';const f=document.querySelector('form');f.requestSubmit();f.firstChild.value='changed';").await.unwrap();
    let wire = request(&rx);
    assert!(
        wire.starts_with("GET /base/search?q=hello+world&dup=one&dup=two HTTP/1.1\r\n"),
        "{wire}"
    );
    assert!(
        wire.to_ascii_lowercase()
            .contains("sec-fetch-dest: document\r\n"),
        "{wire}"
    );
    assert!(
        wire.to_ascii_lowercase().contains("cookie: before=yes\r\n"),
        "{wire}"
    );
    let actual:serde_json::Value=serde_json::from_str(&page.eval_json_expression("({url:location.href,referrer:document.referrer,loaded,old:typeof oldGlobal,cookie:document.cookie,text:document.getElementById('answer').textContent})").await.unwrap()).unwrap();
    assert_eq!(
        actual["url"],
        format!("{base}/base/search?q=hello+world&dup=one&dup=two#kept")
    );
    assert_eq!(actual["referrer"], format!("{base}/start"));
    assert_eq!(actual["loaded"], "response");
    assert_eq!(actual["old"], "undefined");
    assert_eq!(actual["cookie"], "before=yes");
    assert_eq!(actual["text"], "ok");
    assert!(page.cookies().contains("private"));
}

#[tokio::test(flavor = "current_thread")]
async fn post_redirect_carries_real_body_cookies_and_referrer_policy() {
    let (base,rx)=server(vec![response("303 See Other","Location: /done\r\nSet-Cookie: consent=yes; Path=/\r\nReferrer-Policy: no-referrer\r\n",""),response("200 OK","","<h1>done</h1>")]);
    let mut page = PageSession::open(
        "<form method='post' action='/save?old=1'><input name='token' value='a+b c'></form>",
        &format!("{base}/start"),
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval("document.querySelector('form').requestSubmit()")
        .await
        .unwrap();
    let post = request(&rx);
    let get = request(&rx);
    assert!(post.starts_with("POST /save?old=1 HTTP/1.1\r\n"), "{post}");
    assert!(post.ends_with("token=a%2Bb+c"), "{post}");
    assert!(
        post.to_ascii_lowercase()
            .contains("content-type: application/x-www-form-urlencoded\r\n"),
        "{post}"
    );
    assert!(get.starts_with("GET /done HTTP/1.1\r\n"), "{get}");
    assert!(
        get.to_ascii_lowercase().contains("cookie: consent=yes\r\n"),
        "{get}"
    );
    assert!(!get.to_ascii_lowercase().contains("\r\nreferer:"), "{get}");
    assert_eq!(
        page.eval_expression(
            "location.pathname+'|'+document.referrer+'|'+document.body.textContent"
        )
        .await
        .unwrap(),
        "/done||done"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn csp_or_cancelled_form_never_contacts_action() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut page = PageSession::open_with_response_headers(
        "<form action='/blocked'><input name='q' value='x'></form>",
        &format!("{base}/start"),
        "",
        "",
        5000,
        "",
        Default::default(),
        &[(
            "content-security-policy".into(),
            "form-action 'none'".into(),
        )],
    )
    .await
    .unwrap();
    page.eval("globalThis.submits=0;document.querySelector('form').addEventListener('submit',()=>submits++);document.querySelector('form').requestSubmit()").await.unwrap();
    assert_eq!(
        page.eval_expression("submits+'|'+location.pathname")
            .await
            .unwrap(),
        "1|/start"
    );
    assert!(listener.accept().is_err());
    let mut page = PageSession::open(
        "<form action='/blocked'><input name='q' value='x'></form>",
        &format!("{base}/start"),
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval("document.querySelector('form').addEventListener('submit',e=>e.preventDefault());document.querySelector('form').requestSubmit()").await.unwrap();
    assert_eq!(
        page.eval_expression("location.pathname").await.unwrap(),
        "/start"
    );
    assert!(listener.accept().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn initial_script_form_navigation_reaches_real_new_document() {
    let (base, rx) = server(vec![response("200 OK", "", "<p>arrived</p>")]);
    let mut page=PageSession::open("<form action='/initial'><input name='a' value='1'></form><script>document.querySelector('form').requestSubmit()</script>",&format!("{base}/start"),"","",5000).await.unwrap();
    assert!(request(&rx).starts_with("GET /initial?a=1 HTTP/1.1\r\n"));
    assert_eq!(
        page.eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "arrived"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn private_file_metadata_matches_fresh_chrome_control() {
    let mut page = PageSession::open("<body>", "https://forms.invalid/", "", "", 5000)
        .await
        .unwrap();
    let actual = page
        .eval_json_expression(include_str!(
            "../../../../harness/detection/file-metadata-probe.js"
        ))
        .await
        .unwrap();
    let fixture = chrome_fixture(
        include_str!("../../../../harness/detection/fixtures/file-metadata-chrome155.json"),
        include_str!("../../../../harness/detection/file-metadata-probe.js"),
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        fixture["result"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn last_queued_form_wins_and_cancelled_replacement_keeps_first() {
    let (base, rx) = server(vec![
        response("200 OK", "", "<p>last</p>"),
        response("200 OK", "", "<p>first</p>"),
    ]);
    let mut page=PageSession::open("<form id='a' action='/first'><input name='v' value='1'></form><form id='b' action='/last'><input name='v' value='2'></form>",&format!("{base}/start"),"","",5000).await.unwrap();
    page.eval("document.getElementById('a').requestSubmit();document.getElementById('b').requestSubmit();document.getElementById('b').firstChild.value='mutated'").await.unwrap();
    assert!(request(&rx).starts_with("GET /last?v=2 HTTP/1.1\r\n"));
    assert_eq!(
        page.eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "last"
    );
    let mut second=PageSession::open("<form id='a' action='/first'><input name='v' value='1'></form><form id='b' action='/last'></form>",&format!("{base}/start"),"","",5000).await.unwrap();
    second.eval("document.getElementById('a').requestSubmit();document.getElementById('b').addEventListener('submit',e=>e.preventDefault());document.getElementById('b').requestSubmit()").await.unwrap();
    assert!(request(&rx).starts_with("GET /first?v=1 HTTP/1.1\r\n"));
    assert_eq!(
        second
            .eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "first"
    );
    assert_eq!(
        page.eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "last"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn private_formdata_entries_and_live_iterators_match_chrome() {
    let mut page = PageSession::open("<body>", "https://forms.invalid/", "", "", 5000)
        .await
        .unwrap();
    let actual = page
        .eval_json_expression(include_str!(
            "../../../../harness/detection/formdata-private-probe.js"
        ))
        .await
        .unwrap();
    let fixture = chrome_fixture(
        include_str!("../../../../harness/detection/fixtures/formdata-private-chrome155.json"),
        include_str!("../../../../harness/detection/formdata-private-probe.js"),
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        fixture["result"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_formdata_constructor_conversion_order_matches_chrome() {
    let mut page = PageSession::open("<body>", "https://forms.invalid/", "", "", 5000)
        .await
        .unwrap();
    let actual = page
        .eval_json_expression(include_str!(
            "../../../../harness/detection/formdata-constructor-probe.js"
        ))
        .await
        .unwrap();
    let fixture = chrome_fixture(
        include_str!("../../../../harness/detection/fixtures/formdata-constructor-chrome155.json"),
        include_str!("../../../../harness/detection/formdata-constructor-probe.js"),
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        fixture["result"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_base_target_never_silently_routes_to_current_document() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut page = PageSession::open(
        "<base target='_blank'><form action='/must-not-fetch'><input name='q' value='x'></form>",
        &format!("{base}/start"),
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let error=page.eval_expression("(()=>{Object.defineProperty(document,'baseURI',{get(){throw Error('guest base')}});try{document.querySelector('form').requestSubmit();return 'missing failure';}catch(error){return error.message}})()").await.unwrap();
    assert!(error.contains("another browsing context"), "{error}");
    assert_eq!(
        page.eval_expression("location.pathname").await.unwrap(),
        "/start"
    );
    assert!(listener.accept().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn private_formdata_optional_filename_and_file_timestamp_match_chrome() {
    let probe = include_str!("../../../../harness/detection/formdata-optional-probe.js");
    let control = chrome_fixture(
        include_str!("../../../../harness/detection/fixtures/formdata-optional-chrome155.json"),
        probe,
    );
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
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
    assert_eq!(actual, control["result"]);
    page.close();
}
