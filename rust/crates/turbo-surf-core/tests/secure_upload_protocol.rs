use turbo_surf_core::http_auth::Credentials;

#[test]
fn http_credentials_reject_origins_without_an_http_origin() {
    for origin in [
        "file:///tmp/credentials",
        "data:text/plain,credentials",
        "not a URL",
    ] {
        let credentials = Credentials {
            username: "fixture-user".into(),
            password: "fixture-password".into(),
            origin: Some(origin.into()),
        };
        assert_eq!(
            credentials.validate(),
            Err("HTTP credential origin must be an HTTP(S) origin".into())
        );
    }
}

#[cfg(feature = "impersonate")]
#[tokio::test]
async fn a_trusted_tls_peer_without_http2_cannot_receive_a_streamed_upload() {
    use std::sync::Arc;
    use tokio::io::AsyncReadExt;
    use turbo_surf_core::{
        net::{self, ErrorCode, FetchOptions},
        request_context::ClientHints,
    };

    const CERT: &[u8] = include_bytes!("../../turbo-surf-render/tests/csp_report_cert.der");
    const KEY: &[u8] = include_bytes!("../../turbo-surf-render/tests/csp_report_key.der");
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![rustls::pki_types::CertificateDer::from(CERT.to_vec())],
        rustls::pki_types::PrivatePkcs8KeyDer::from(KEY.to_vec()).into(),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("https://{}", listener.local_addr().unwrap());
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(socket).await.unwrap();
        assert!(stream.get_ref().1.alpn_protocol().is_none());
        let mut byte = [0];
        // TLS alone is trusted; upload admission must stop before any HTTP bytes.
        match stream.read(&mut byte).await {
            Ok(0) => (),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                ()
            }
            outcome => panic!("HTTP/1 peer received upload traffic: {outcome:?}"),
        }
    });
    let hints = ClientHints::with_client_builder(|| net::client_builder_with_trust(CERT));
    let (_sender, upload) = turbo_surf_core::upload::upload_channel(16);
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let result = net::fetch_stream(
            &format!("{origin}/upload"),
            FetchOptions {
                method: Some("POST".into()),
                upload_body: Some(upload),
                client_hints: Some(&hints),
                ..Default::default()
            },
        )
        .await;
        peer.await.unwrap();
        result
    })
    .await
    .unwrap();
    let error = result
        .err()
        .expect("HTTP/1 upload must fail before sending a request");
    assert_eq!(error.code, ErrorCode::Network);
    assert!(
        error.message.contains("Streaming requests require HTTP/2"),
        "{}",
        error.message
    );
}
