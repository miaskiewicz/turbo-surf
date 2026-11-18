//! Trusted screenshot assets follow actual document ownership, not guest serializers.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAYAAACddGYaAAAAEUlEQVR4nGP4z8DwH4YZkDkAm34L9XKwuTwAAAAASUVORK5CYII=";
const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"7\" height=\"5\"/>";

struct Site {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn site() -> Site {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let observed = observed.clone();
            tokio::spawn(async move {
                let mut request = vec![0; 16 * 1024];
                let mut size = 0;
                loop {
                    let count = stream.read(&mut request[size..]).await.unwrap();
                    assert!(count > 0, "image fixture request ended before its headers");
                    size += count;
                    if request[..size].windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                    assert!(size < request.len(), "image fixture headers exceed 16 KiB");
                }
                let path = String::from_utf8_lossy(&request[..size])
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                observed.lock().unwrap().push(path.clone());
                let (status, mime, extra, body) = match path.as_str() {
                    "/redirect" => (
                        "302 Found",
                        "text/plain",
                        "Location: /image.png\r\n",
                        Vec::new(),
                    ),
                    "/image.png" => ("200 OK", "image/png", "", STANDARD.decode(PNG).unwrap()),
                    "/blue.svg" => ("200 OK", "image/svg+xml", "", SVG.as_bytes().to_vec()),
                    _ => ("404 Not Found", "text/plain", "", b"missing".to_vec()),
                };
                let head = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{extra}\r\n", body.len());
                stream.write_all(head.as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
            });
        }
    });
    Site {
        base,
        requests,
        task,
    }
}
async fn page(site: &Site) -> PageSession {
    let mut page = PageSession::open(
        "<body><img id=image src=/redirect>",
        &site.base,
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    assert_eq!(
        page.eval_json_expression("[image.complete,image.naturalWidth,image.naturalHeight]")
            .await
            .unwrap(),
        "[true,3,2]"
    );
    page
}
fn assets(page: &mut PageSession, target: u64, query: Value) -> Value {
    serde_json::from_str(
        &page
            .screenshot_target_image_assets(target, &query.to_string())
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn retained_image_snapshot_preserves_redirect_bytes_and_generation_cache() {
    let site = site().await;
    let mut page = page(&site).await;
    let target = page.auxiliary_root_id().unwrap();
    let refs = json!([
        "/redirect",
        format!("{}/redirect", site.base),
        format!("{}/image.png", site.base),
        "/absent"
    ]);
    let first = assets(&mut page, target, json!({"refs":refs}));
    assert!(first["generation"].as_str().is_some());
    let images = first["images"].as_object().unwrap();
    assert_eq!(images.len(), 3);
    assert!(images.values().all(|value| value == PNG));
    assert_eq!(*site.requests.lock().unwrap(), ["/redirect", "/image.png"]);
    let cached = assets(
        &mut page,
        target,
        json!({"refs":refs,"generation":first["generation"],"cachedRefs":refs}),
    );
    assert_eq!(cached["generation"], first["generation"]);
    assert_eq!(cached["images"], json!({}));
    let stale = assets(
        &mut page,
        target,
        json!({"refs":refs,"generation":"stale","cachedRefs":refs}),
    );
    assert_eq!(stale, first);
    page.eval_expression(
        "JSON.stringify=()=>{throw new Error('guest serializer must not run')};void 0",
    )
    .await
    .unwrap();
    assert_eq!(assets(&mut page, target, json!({"refs":refs})), first);
    assert_eq!(*site.requests.lock().unwrap(), ["/redirect", "/image.png"]);
}

#[tokio::test(flavor = "current_thread")]
async fn resident_image_snapshot_rejects_old_sources_and_detached_nodes() {
    let site = site().await;
    let mut page = page(&site).await;
    let target = page.auxiliary_root_id().unwrap();
    let before = assets(&mut page, target, json!({"refs":["/redirect"]}));
    assert_eq!(page.eval_json_expression("new Promise((resolve,reject)=>{image.onload=()=>resolve([image.naturalWidth,image.naturalHeight]);image.onerror=reject;image.src='/blue.svg'})").await.unwrap(), "[7,5]");
    let changed = assets(
        &mut page,
        target,
        json!({"refs":["/redirect","/blue.svg"],"generation":before["generation"],"cachedRefs":["/redirect","/blue.svg"]}),
    );
    assert_ne!(changed["generation"], before["generation"]);
    assert_eq!(changed["images"], json!({"/blue.svg":STANDARD.encode(SVG)}));
    page.eval_expression("image.remove();void 0").await.unwrap();
    assert_eq!(
        assets(&mut page, target, json!({"refs":["/blue.svg"]}))["images"],
        json!({})
    );
    page.set_content("<body>replacement", "load", Some(10000))
        .await
        .unwrap();
    let cleared = assets(&mut page, target, json!({"refs":["/redirect","/blue.svg"]}));
    assert_ne!(cleared["generation"], changed["generation"]);
    assert_eq!(cleared["images"], json!({}));
    assert_eq!(
        *site.requests.lock().unwrap(),
        ["/redirect", "/image.png", "/blue.svg"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn root_asset_api_matches_its_actual_target_without_guest_execution() {
    let site = site().await;
    let mut page = page(&site).await;
    let target = page.auxiliary_root_id().unwrap();
    let query = json!({"refs":["/redirect"]});
    let expected = assets(&mut page, target, query.clone());
    assert_eq!(expected["images"], json!({"/redirect":PNG}));
    let actual: Value =
        serde_json::from_str(&page.screenshot_image_assets(&query.to_string()).unwrap()).unwrap();
    assert_eq!(actual, expected);
    assert!(page
        .screenshot_target_image_assets(u64::MAX, &query.to_string())
        .is_err());
    assert!(page
        .screenshot_target_image_assets(target, "{invalid")
        .is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn resident_asset_queries_keep_live_peer_documents_separate() {
    let site = site().await;
    let mut page = page(&site).await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval_expression("globalThis.peer=window.open('about:blank','image-peer');void 0")
        .await
        .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|entry| entry.target != root.to_string())
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    assert_eq!(
        assets(&mut page, peer, json!({"refs":["/redirect"]}))["images"],
        json!({})
    );
    let script=format!("new Promise((resolve,reject)=>{{const image=document.createElement('img');image.onload=()=>resolve([image.naturalWidth,image.naturalHeight]);image.onerror=reject;image.src={};document.body.append(image);}})",json!(format!("{}/blue.svg",site.base)));
    assert_eq!(
        page.eval_target_expression(peer, &script, false, true)
            .await
            .unwrap(),
        "[7,5]"
    );
    let peer_ref = format!("{}/blue.svg", site.base);
    let peer_assets = assets(&mut page, peer, json!({"refs":[peer_ref,"/redirect"]}));
    assert_eq!(
        peer_assets["images"],
        json!({(peer_ref.clone()):STANDARD.encode(SVG)})
    );
    assert_eq!(
        assets(&mut page, root, json!({"refs":[peer_ref]}))["images"],
        json!({})
    );
    page.eval_expression("peer.close();void 0").await.unwrap();
    assert!(page
        .screenshot_target_image_assets(peer, &json!({"refs":[peer_ref]}).to_string())
        .is_err());
    assert_eq!(
        assets(&mut page, root, json!({"refs":["/redirect"]}))["images"],
        json!({"/redirect":PNG})
    );
}
