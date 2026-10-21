use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use turbo_surf_core::{net, request_context::ClientHints};
use turbo_surf_render::PageSession;

struct Fixture(Child);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn open_fixture() -> (Fixture, PageSession) {
    let server = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../harness/detection/upload-server.mjs")
        .canonicalize()
        .expect("upload fixture source must exist in ordinary and linked test workspaces");
    let child = Command::new("node")
        .arg(server)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut fixture = Fixture(child);
    let mut line = String::new();
    BufReader::new(fixture.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(
        !line.is_empty(),
        "upload fixture exited before announcing its localhost endpoints"
    );
    let urls: serde_json::Value = serde_json::from_str(&line).unwrap();
    let init = format!(
        "globalThis.__uploadPeer={};globalThis.__uploadHttp1={};",
        urls["peer"], urls["http1"]
    );
    let hints = ClientHints::with_client_builder(|| {
        net::client_builder_with_trust(include_bytes!("csp_report_cert.der"))
    });
    let page = PageSession::open_with_request_state(
        "<!doctype html><body>",
        urls["primary"].as_str().unwrap(),
        "",
        "",
        20000,
        &init,
        hints,
    )
    .await
    .unwrap();
    (fixture, page)
}

#[tokio::test(flavor = "current_thread")]
async fn https_http2_upload_streams_match_source_pinned_chrome() {
    let (_fixture, mut page) = open_fixture().await;
    let probe = include_str!("../../../../harness/detection/upload-probe.js")
        .trim()
        .trim_end_matches(';');
    let result = page.eval_json_expression(probe).await.unwrap();
    let actual: serde_json::Value = serde_json::from_str(&result).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/upload-chrome155.json"
    ))
    .unwrap();
    eprintln!("actual upload result: {actual}");
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn https_http2_buffered_response_does_not_enqueue_empty_transport_frames() {
    let (_fixture, mut page) = open_fixture().await;
    let probe = include_str!("../../../../harness/detection/response-h2-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/response-h2-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn stalled_upload_abort_rejects_before_bounded_transport_cleanup() {
    let (_fixture, mut page) = open_fixture().await;
    let probe = include_str!("../../../../harness/detection/upload-abort-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/upload-abort-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}
