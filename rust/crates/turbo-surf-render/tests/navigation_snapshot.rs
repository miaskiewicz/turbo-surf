use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};
use turbo_surf_render::PageSession;
fn server(responses: Vec<String>) -> (String, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(60)))
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
    rx.recv_timeout(std::time::Duration::from_secs(60)).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn same_url_post_snapshot_tracks_installed_response_and_private_cookies() {
    let (base, rx) = server(vec![response(
        "201 Created",
        "Set-Cookie: private=yes; HttpOnly; Path=/\r\n",
        "<p id='actual'>response</p>",
    )]);
    let url = format!("{base}/same");
    let mut page = PageSession::open(
        "<form method='post'><input name='q' value='same'></form>",
        &url,
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let initial = page.navigation_snapshot();
    assert_eq!(initial.0, 0);
    assert_eq!(initial.1, url);
    page.eval("document.querySelector('form').submit()")
        .await
        .unwrap();
    let installed = page.navigation_snapshot();
    assert_eq!(installed.0, 1);
    assert_eq!(installed.1, url);
    assert_eq!(installed.2.status, 201);
    assert_eq!(installed.2.referrer, url);
    assert!(request(&rx).starts_with("POST /same HTTP/1.1\r\n"));
    assert!(page.serialize().contains("response"));
    assert!(page.cookies().contains("private"));
    page.eval("Object.defineProperty(document,'referrer',{value:'https://spoof.invalid/'});globalThis.__RESULT='spoof';JSON.stringify=()=> 'spoof';").await.unwrap();
    assert_eq!(page.navigation_snapshot().2.referrer, url);
    assert_eq!(page.navigation_snapshot().2.status, 201);
    page.close();
}
#[tokio::test(flavor = "current_thread")]
async fn canonical_trusted_submit_event_keeps_global_constructor_brand() {
    let mut page = PageSession::open(
        "<form><button id='b'>go</button></form>",
        "https://example.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result=page.eval_json_expression("(()=>{let out;const f=document.querySelector('form');f.addEventListener('submit',e=>{e.preventDefault();out=[e instanceof SubmitEvent,Object.getOwnPropertyDescriptor(SubmitEvent.prototype,'submitter').get.call(e)===document.getElementById('b'),e.isTrusted]});f.requestSubmit(document.getElementById('b'));return out})()").await.unwrap();
    assert_eq!(result, "[true,true,true]");
    assert_eq!(page.navigation_snapshot().0, 0);
    page.close();
}
