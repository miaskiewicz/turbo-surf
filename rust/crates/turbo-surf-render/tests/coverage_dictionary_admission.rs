//! Project dictionary admission caps and URL policy over actual localhost TLS responses.
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use turbo_surf_core::{
    net::{self, FetchOptions},
    request_context::ClientHints,
};
use turbo_surf_render::PageSession;

struct Site {
    child: Child,
    origin: String,
}
impl Drop for Site {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Site {
    fn start() -> Self {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../harness/detection/compression-dictionary-server.mjs");
        let mut child = Command::new("node")
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let endpoints: Value = serde_json::from_str(&line).unwrap();
        Self {
            child,
            origin: endpoints["primary"].as_str().unwrap().into(),
        }
    }
    fn dictionary(&self, slot: usize) -> String {
        format!("{}/dictionary?case=8&slot={slot}", self.origin)
    }
    async fn wire(&self) -> Vec<Value> {
        let response = net::fetch_html(
            &format!("{}/wire", self.origin),
            FetchOptions {
                client_hints: Some(&hints()),
                allow_non_html: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        serde_json::from_str(&response.html).unwrap()
    }
}
fn hints() -> ClientHints {
    ClientHints::with_client_builder(|| {
        net::client_builder_with_trust(include_bytes!("csp_report_cert.der"))
    })
}
fn document(links: &[(String, String)]) -> String {
    let links = links
        .iter()
        .map(|(id, href)| format!("<link id={id} rel=compression-dictionary href='{href}'>"))
        .collect::<String>();
    format!(
        r#"<!doctype html><head><script>
        globalThis.events=[];globalThis.originalDocument=document;
        globalThis.marker={{value:7}};globalThis.originalMarker=marker;
        const record=event=>{{if(event.target.localName==='link')events.push([event.target.id,event.type])}};
        document.addEventListener('load',record,true);document.addEventListener('error',record,true);
        globalThis.waitDictionaryEvents=count=>{{
          if(events.length>=count)return Promise.resolve(events.slice());
          return new Promise(resolve=>{{const changed=()=>{{if(events.length>=count){{
            document.removeEventListener('load',changed,true);document.removeEventListener('error',changed,true);
            resolve(events.slice());
          }}}};document.addEventListener('load',changed,true);document.addEventListener('error',changed,true)}});
        }};
        </script>{links}</head><body>dictionary admission</body>"#
    )
}
async fn open(html: &str, source: &str) -> PageSession {
    PageSession::open_with_response_headers(html, source, "", "", 20_000, "", hints(), &[])
        .await
        .unwrap()
}
async fn events(page: &mut PageSession, count: usize) -> Vec<Value> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        page.eval_json_expression(&format!("waitDictionaryEvents({count})")),
    )
    .await
    .expect("actual admitted dictionary responses must complete")
    .unwrap();
    serde_json::from_str(&output).unwrap()
}
fn dictionary_responses(page: &mut PageSession) -> Vec<Value> {
    let rows: Vec<Value> = serde_json::from_str(&page.drain_network_log()).unwrap();
    rows.into_iter()
        .filter(|row| {
            row["event"] == "response"
                && url::Url::parse(row["url"].as_str().unwrap())
                    .unwrap()
                    .path()
                    == "/dictionary"
        })
        .collect()
}
fn assert_actual_dictionary_bodies(rows: &[Value]) {
    assert!(rows.iter().all(|row| {
        row["status"] == 200 && row["body"] == "authored reusable compression dictionary case 8"
    }));
}
async fn assert_healthy_quiescent(page: &mut PageSession) {
    page.poll_owned_work().await.unwrap();
    let root = page.auxiliary_root_id().unwrap().to_string();
    assert_eq!(
        page.network_event_log().target_network_state(&root)["pendingRequests"],
        0
    );
    assert_eq!(
        page.eval_json_expression(
            "[document===originalDocument,marker===originalMarker,marker.value,document.body.textContent]"
        ).await.unwrap(),
        r#"[true,true,7,"dictionary admission"]"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn distinct_dictionary_cap_denies_the_next_target_but_preserves_reuse() {
    let site = Site::start();
    let links = (0..65)
        .map(|slot| (format!("d{slot}"), site.dictionary(slot)))
        .collect::<Vec<_>>();
    let mut page = open(&document(&links), &format!("{}/page", site.origin)).await;
    let first = events(&mut page, 64).await;
    assert_eq!(first.len(), 64);
    let expected = (0..64)
        .map(|slot| json!([format!("d{slot}"), "load"]).to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        first.iter().map(Value::to_string).collect::<BTreeSet<_>>(),
        expected
    );
    assert_healthy_quiescent(&mut page).await;
    let responses = dictionary_responses(&mut page);
    assert_eq!(responses.len(), 64);
    assert_actual_dictionary_bodies(&responses);
    let requested = responses
        .iter()
        .map(|row| row["url"].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        requested,
        (0..64).map(|slot| site.dictionary(slot)).collect()
    );
    let wire = site.wire().await;
    assert_eq!(wire.len(), 64);
    assert!(wire.iter().all(|row| {
        row["path"] == "/dictionary" && row["finished"].is_number() && row["aborted"] == false
    }));

    page.eval_expression(&format!(
        "document.getElementById('d64').href={};void 0",
        json!(site.dictionary(0))
    ))
    .await
    .unwrap();
    let reused = events(&mut page, 65).await;
    assert_eq!(reused.len(), 65);
    assert_eq!(reused.last().unwrap(), &json!(["d64", "load"]));
    assert_healthy_quiescent(&mut page).await;
    assert!(dictionary_responses(&mut page)
        .iter()
        .all(|row| row["url"] == site.dictionary(0)));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_dictionary_targets_are_not_admitted_and_href_repair_recovers() {
    let site = Site::start();
    let credentialed = site
        .dictionary(0)
        .replacen("https://", "https://user:pass@", 1);
    let links = vec![
        ("credentials".into(), credentialed),
        (
            "http".into(),
            site.dictionary(1).replacen("https://", "http://", 1),
        ),
        ("data".into(), "data:text/plain,untrusted dictionary".into()),
        ("valid".into(), site.dictionary(2)),
    ];
    let mut page = open(&document(&links), &format!("{}/page", site.origin)).await;
    assert_eq!(events(&mut page, 1).await, vec![json!(["valid", "load"])]);
    assert_healthy_quiescent(&mut page).await;
    let first = dictionary_responses(&mut page);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["url"], site.dictionary(2));
    assert_actual_dictionary_bodies(&first);
    assert_eq!(site.wire().await.len(), 1);
    page.eval_expression(&format!(
        "document.getElementById('credentials').href={};void 0",
        json!(site.dictionary(3))
    ))
    .await
    .unwrap();
    assert_eq!(
        events(&mut page, 2).await,
        vec![json!(["valid", "load"]), json!(["credentials", "load"])]
    );
    assert_healthy_quiescent(&mut page).await;
    let repaired = dictionary_responses(&mut page);
    assert_eq!(repaired.len(), 1);
    assert_eq!(repaired[0]["url"], site.dictionary(3));
    assert_actual_dictionary_bodies(&repaired);
    assert_eq!(site.wire().await.len(), 2);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn insecure_dictionary_source_is_quiescent_while_https_source_fetches_real_bytes() {
    let site = Site::start();
    let html = document(&[("dictionary".into(), site.dictionary(0))]);
    let insecure = format!("{}/page", site.origin.replacen("https://", "http://", 1));
    let mut page = open(&html, &insecure).await;
    assert_healthy_quiescent(&mut page).await;
    assert_eq!(page.eval_json_expression("events").await.unwrap(), "[]");
    assert!(dictionary_responses(&mut page).is_empty());
    assert!(site.wire().await.is_empty());
    page.close();

    let mut secure = open(&html, &format!("{}/page", site.origin)).await;
    assert_eq!(
        events(&mut secure, 1).await,
        vec![json!(["dictionary", "load"])]
    );
    assert_healthy_quiescent(&mut secure).await;
    let admitted = dictionary_responses(&mut secure);
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0]["url"], site.dictionary(0));
    assert_actual_dictionary_bodies(&admitted);
    assert_eq!(site.wire().await.len(), 1);
    secure.close();
}
