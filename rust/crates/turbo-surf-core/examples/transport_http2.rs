//! Localhost HTTP/2 wire control with an explicit fixture certificate store.
#[cfg(feature = "impersonate")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use turbo_surf_core::net::{client_builder, fetch_html, FetchOptions};
    let args: Vec<_> = std::env::args().collect();
    let url = args.get(1).ok_or("localhost URL required")?;
    let parsed = url::Url::parse(url)?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("127.0.0.1") {
        return Err("transport fixture requires localhost HTTPS".into());
    }
    let pem = std::fs::read(args.get(2).ok_or("fixture certificate required")?)?;
    let trust = wreq::tls::trust::CertStore::builder()
        .add_pem_cert(pem.as_slice())
        .build()?;
    let client = client_builder().tls_cert_store(trust).build()?;
    let result = fetch_html(
        url,
        FetchOptions {
            client: Some(&client),
            ..Default::default()
        },
    )
    .await?;
    if result.status != 200 {
        return Err(format!("fixture returned HTTP {}", result.status).into());
    }
    println!(
        "{}",
        serde_json::json!({"status":result.status,"transportChromeMajor":turbo_surf_core::net::transport_chrome_major(),"profileMajor":turbo_surf_core::browser::DEFAULT_MAJOR})
    );
    Ok(())
}

#[cfg(not(feature = "impersonate"))]
fn main() {
    eprintln!("The HTTP/2 transport fixture requires the impersonate feature");
    std::process::exit(1);
}
