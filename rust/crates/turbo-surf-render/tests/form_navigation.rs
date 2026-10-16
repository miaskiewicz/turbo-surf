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
async fn reflected_submitter_overrides_drive_actual_post_navigation() {
    for tag in ["button", "input"] {
        let (base, rx) = server(vec![response("200 OK", "", "<p>submitted</p>")]);
        let html = format!(
            "<base href='/base/' target='_blank'><form action='/wrong' method='get'><input name='field' value='hello world'><{tag} type='submit' name='chosen' value='{tag}'></{tag}></form>"
        );
        let mut page = PageSession::open(&html, &format!("{base}/start"), "", "", 60_000)
            .await
            .unwrap();
        page.eval(
            r#"const form=document.querySelector('form'),submitter=document.querySelector('[name=chosen]');
            Element.prototype.setAttribute=function(){throw Error('guest attribute method')};
            submitter.formAction='submitter';submitter.formMethod='PoSt';
            submitter.formEnctype='text/plain';submitter.formTarget='_self';
            form.requestSubmit(submitter);"#,
        )
        .await
        .unwrap();
        let wire = request(&rx);
        assert!(
            wire.starts_with("POST /base/submitter HTTP/1.1\r\n"),
            "{wire}"
        );
        assert!(
            wire.to_ascii_lowercase()
                .contains("content-type: text/plain\r\n"),
            "{wire}"
        );
        assert!(
            wire.ends_with(&format!("field=hello world\r\nchosen={tag}\r\n")),
            "{wire}"
        );
        assert_eq!(
            page.eval_expression("location.pathname+'|'+document.body.textContent")
                .await
                .unwrap(),
            "/base/submitter|submitted"
        );
    }
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

#[tokio::test(flavor = "current_thread")]
async fn navigation_activation_comes_from_private_input_and_expires_per_document() {
    for action in [
        "submit",
        "requestSubmit",
        "untrusted",
        "click",
        "enter",
        "handler",
        "pointerdown",
        "canceled-pointerdown",
        "expired",
    ] {
        let (base, rx) = server(vec![
            response("302 Found", "Location: /destination\r\n", ""),
            response("200 OK", "", "<form action='/next'></form>"),
            response("200 OK", "", "<p>next</p>"),
        ]);
        let mut page = PageSession::open(
            "<form action='/destination'><input id=q name=q><button id=go>go</button></form>",
            &format!("{base}/start"),
            "",
            "",
            10000,
        )
        .await
        .unwrap();
        match action {
            "submit" | "requestSubmit" => {
                page.eval(&format!("document.querySelector('form').{action}()"))
                    .await
                    .unwrap();
            }
            "untrusted" => {
                page.eval("document.querySelector('#go').dispatchEvent(new MouseEvent('mousedown',{bubbles:true}));document.querySelector('form').requestSubmit()") .await.unwrap();
            }
            "click" => page
                .input(&serde_json::json!([{"click":"#go"}]))
                .await
                .unwrap(),
            "enter" => page
                .input(&serde_json::json!([{"focus":"#q"},{"press":"Enter"}]))
                .await
                .unwrap(),
            "handler" => {
                page.eval("document.querySelector('#q').addEventListener('keydown',()=>document.querySelector('form').submit())").await.unwrap();
                page.input(&serde_json::json!([{"focus":"#q"},{"press":"x"}]))
                    .await
                    .unwrap();
            }
            "pointerdown" | "canceled-pointerdown" => {
                page.eval(&format!(
                    "document.querySelector('#go').addEventListener('pointerdown',event=>{{{}document.querySelector('form').submit()}})",
                    if action == "canceled-pointerdown" { "event.preventDefault();" } else { "" },
                )).await.unwrap();
                page.input(&serde_json::json!([{"click":"#go"}]))
                    .await
                    .unwrap();
            }
            "expired" => {
                page.input(&serde_json::json!([{"focus":"#q"},{"press":"x"}]))
                    .await
                    .unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(5100)).await;
                page.eval("document.querySelector('form').submit()")
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let activated = matches!(
            action,
            "click" | "enter" | "handler" | "pointerdown" | "canceled-pointerdown"
        );
        for _ in 0..2 {
            let wire = request(&rx).to_ascii_lowercase();
            assert_eq!(
                wire.contains("sec-fetch-user: ?1\r\n"),
                activated,
                "{action}"
            );
            assert!(wire.contains("sec-fetch-site: same-origin\r\n"), "{action}");
            assert!(wire.contains("sec-fetch-mode: navigate\r\n"), "{action}");
        }
        // Replacing the document retires activation; a script cannot inherit it.
        page.eval("document.querySelector('form').submit()")
            .await
            .unwrap();
        assert!(
            !request(&rx)
                .to_ascii_lowercase()
                .contains("sec-fetch-user:"),
            "{action}"
        );
        page.close();
    }
}
