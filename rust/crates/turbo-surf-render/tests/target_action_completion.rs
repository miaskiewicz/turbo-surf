//! Driver actions finish actual native defaults without replaying static intents.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn target_actions_await_get_post_and_shadow_anchor_documents() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (send, received) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut requests = Vec::new();
        while requests.len() < 3 && Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0; 4096];
            let size = stream.read(&mut bytes).unwrap();
            let wire = String::from_utf8_lossy(&bytes[..size]).into_owned();
            requests.push(wire.lines().next().unwrap().to_owned());
            let body = "<!doctype html><title>Destination</title><p>new document</p>";
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        send.send(requests).unwrap();
    });
    let cases = [
        ("<form action='/get'><input name='q' value='rust'><button>Go</button></form>", "document.querySelector('button').click()", "GET /get?q=rust HTTP/1.1"),
        ("<form action='/start' method='post'><input name='q' value='rust'><button>Go</button></form>", "document.querySelector('button').click()", "POST /start HTTP/1.1"),
        ("<base href='/folder/'><div id='host'></div>", "const root=document.querySelector('#host').attachShadow({mode:'open'}); root.innerHTML='<a href=next><span>Next</span></a>';root.querySelector('span').click()", "GET /folder/next HTTP/1.1"),
    ];
    let mut expected = Vec::new();
    for (html, action, request) in cases {
        let mut page = PageSession::open(html, &format!("{origin}/start"), "", "", 5000)
            .await
            .unwrap();
        let target = page.auxiliary_root_id().unwrap();
        assert!(
            !page.target_is_primary(target).unwrap(),
            "actual guest is distinct from infrastructure"
        );
        let script = format!(
            "globalThis.oldDocumentMarker=true; {action};globalThis.__RESULT='dispatch-result';"
        );
        assert_eq!(
            page.dispatch_target_engine(target, &script).await.unwrap(),
            "dispatch-result"
        );
        assert!(page
            .serialize_target(target)
            .unwrap()
            .contains("new document"));
        assert_eq!(
            page.read_target_engine(target, "return typeof globalThis.oldDocumentMarker;")
                .unwrap(),
            "\"undefined\""
        );
        assert_eq!(
            page.auxiliary_root_id().unwrap(),
            target,
            "retain actual target identity"
        );
        expected.push(request.to_owned());
        page.close();
    }
    assert_eq!(
        received.recv_timeout(Duration::from_secs(2)).unwrap(),
        expected
    );
    server.join().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn target_action_leaves_sibling_form_and_location_navigation_pending() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        while requests.load(Ordering::SeqCst) < 4 {
            assert!(
                Instant::now() < deadline,
                "owned navigation never contacted fixture"
            );
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0; 4096];
            let size = stream.read(&mut bytes).unwrap();
            let wire = String::from_utf8_lossy(&bytes[..size]);
            let first = wire.lines().next().unwrap();
            assert!(
                first == "POST /root HTTP/1.1"
                    || first == "POST /child HTTP/1.1"
                    || first == "GET /child HTTP/1.1",
                "{wire}"
            );
            requests.fetch_add(1, Ordering::SeqCst);
            let body = if first.contains("/root") {
                "<!doctype html><p>root replacement</p>"
            } else {
                "<!doctype html><p>child replacement</p>"
            };
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        }
    });
    for mode in ["form", "location"] {
        let before = count.load(Ordering::SeqCst);
        let mut page = PageSession::open(
            "<form method='post' action='/root'></form><p>root retained</p>",
            &origin,
            "",
            "",
            5000,
        )
        .await
        .unwrap();
        let root = page.auxiliary_root_id().unwrap();
        page.eval("window.open('about:blank','spare')")
            .await
            .unwrap();
        let child: u64 = page
            .auxiliary_event_log()
            .pages()
            .into_iter()
            .find(|entry| entry.target != root.to_string())
            .unwrap()
            .target
            .parse()
            .unwrap();
        let baseline = page
            .network_event_log()
            .target_network_state(&child.to_string())["navigationRevision"]
            .as_str()
            .unwrap()
            .to_owned();
        let admission = if mode == "form" {
            format!("document.body.innerHTML='<form method=post action=\"{origin}/child\"></form>'; document.forms[0].submit();")
        } else {
            format!("location.href='{origin}/child';")
        };
        page.eval_target_engine(
            child,
            &format!("{admission} globalThis.__RESULT='admitted';"),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            count.load(Ordering::SeqCst),
            before,
            "generic evaluation does not auto-wait a target navigation"
        );
        assert_eq!(
            page.dispatch_target_engine(
                root,
                "document.forms[0].submit();globalThis.__RESULT='root-result';"
            )
            .await
            .unwrap(),
            "root-result"
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            before + 1,
            "root action must not admit pending sibling {mode}"
        );
        assert!(page
            .serialize_target(root)
            .unwrap()
            .contains("root replacement"));
        assert_eq!(
            page.dispatch_target_engine(child, "globalThis.__RESULT='child-result';")
                .await
                .unwrap(),
            "child-result"
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            before + 1,
            "a new marker action must not join a previously admitted child navigation"
        );
        let outcome: serde_json::Value = serde_json::from_str(
            &page
                .dispatch_target_engine_since(child, "", Some(&baseline))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(outcome["status"], "released");
        assert_eq!(count.load(Ordering::SeqCst), before + 2);
        assert!(page
            .serialize_target(child)
            .unwrap()
            .contains("child replacement"));
        page.close();
    }
    server.join().unwrap();
}
