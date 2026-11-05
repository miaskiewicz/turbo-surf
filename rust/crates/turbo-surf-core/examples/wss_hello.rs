//! Open one localhost `https:` or `wss:` connection for the wss: ClientHello row of the
//! identity battery (harness/detection/wss-hello.mjs). The capture listener closes after
//! the ClientHello, so neither request completes.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let url = std::env::args().nth(1).expect("localhost URL required");
    let parsed = url::Url::parse(&url).expect("invalid URL");
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    match parsed.scheme() {
        "https" => {
            let options = turbo_surf_core::net::FetchOptions::default();
            let _ = turbo_surf_core::net::fetch_html(&url, options).await;
        }
        "wss" => {
            let request = turbo_surf_core::websocket::WsRequest {
                url,
                origin: "https://127.0.0.1".into(),
                ..Default::default()
            };
            let (_handle, mut events) = turbo_surf_core::websocket::open(request);
            let _ = events.recv().await;
        }
        other => panic!("unsupported scheme {other}"),
    }
}
