//! Open a localhost TLS connection for the reusable ClientHello oracle harness.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let url = std::env::args().nth(1).expect("localhost URL required");
    let parsed = url::Url::parse(&url).expect("invalid URL");
    assert_eq!(parsed.scheme(), "https");
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    // The capture server intentionally closes before certificate exchange.
    let _ =
        turbo_surf_core::net::fetch_html(&url, turbo_surf_core::net::FetchOptions::default()).await;
}
