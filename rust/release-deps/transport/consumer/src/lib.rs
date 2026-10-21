#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};
    use wreq::connector::{Connected, Connection};

    struct Metadata;

    impl Connection for Metadata {
        fn connected(&self) -> Connected {
            Connected::new().negotiated_h2()
        }
    }

    #[test]
    fn public_connector_metadata_retains_the_required_h2_seam() {
        assert!(Metadata.connected().is_negotiated_h2());
        assert!(!Connected::new().is_negotiated_h2());
    }

    #[tokio::test]
    async fn profile_client_decodes_an_actual_localhost_response() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "client never connected");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("local accept failed: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let size = socket.read(&mut chunk).unwrap();
                assert_ne!(size, 0);
                request.extend_from_slice(&chunk[..size]);
                if request.windows(4).any(|value| value == b"\r\n\r\n") {
                    break;
                }
                assert!(request.len() <= 16384);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /release-proof HTTP/1.1\r\n"));
            assert!(request
                .to_ascii_lowercase()
                .contains("user-agent: release-consumer\r\n"));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .unwrap();
        });
        let profile = wreq_util::Emulation::builder()
            .profile(wreq_util::Profile::Chrome154)
            .platform(wreq_util::Platform::MacOS)
            .build();
        let client = wreq::Client::builder()
            .emulation(profile)
            .no_proxy()
            .user_agent("release-consumer")
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let response = client
            .get(format!("http://{address}/release-proof"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), "hello");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn profile_client_completes_a_real_local_tls_handshake() {
        use btls::asn1::Asn1Time;
        use btls::bn::BigNum;
        use btls::hash::MessageDigest;
        use btls::pkey::PKey;
        use btls::rsa::Rsa;
        use btls::ssl::{SslAcceptor, SslMethod};
        use btls::x509::{X509NameBuilder, X509};

        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "localhost").unwrap();
        let name = name.build();
        let mut certificate = X509::builder().unwrap();
        certificate.set_version(2).unwrap();
        let serial = BigNum::from_u32(1).unwrap().to_asn1_integer().unwrap();
        certificate.set_serial_number(&serial).unwrap();
        certificate.set_subject_name(&name).unwrap();
        certificate.set_issuer_name(&name).unwrap();
        certificate.set_pubkey(&key).unwrap();
        certificate
            .set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        certificate
            .set_not_after(&Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        certificate.sign(&key, MessageDigest::sha256()).unwrap();
        let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
        acceptor.set_private_key(&key).unwrap();
        acceptor.set_certificate(&certificate.build()).unwrap();
        acceptor.check_private_key().unwrap();
        let acceptor = acceptor.build();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "TLS client never connected");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("local TLS accept failed: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut stream = acceptor.accept(socket).unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|value| value == b"\r\n\r\n") {
                let mut chunk = [0; 1024];
                let size = stream.read(&mut chunk).unwrap();
                assert_ne!(size, 0);
                request.extend_from_slice(&chunk[..size]);
                assert!(request.len() <= 16384);
            }
            assert!(request.starts_with(b"GET /tls-proof HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\ntls")
                .unwrap();
        });
        let profile = wreq_util::Emulation::builder()
            .profile(wreq_util::Profile::Chrome154)
            .platform(wreq_util::Platform::MacOS)
            .build();
        let client = wreq::Client::builder()
            .emulation(profile)
            .no_proxy()
            .tls_cert_verification(false) // The fixture signs its own localhost certificate.
            .tls_verify_hostname(false)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let response = client
            .get(format!("https://{address}/tls-proof"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), "tls");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn native_btls_and_tokio_btls_share_the_same_ssl_type() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (peer, _) = listener.accept().await.unwrap();
        let context = btls::ssl::SslContextBuilder::new(btls::ssl::SslMethod::tls())
            .unwrap()
            .build();
        let ssl = btls::ssl::Ssl::new(&context).unwrap();
        let stream = tokio_btls::SslStream::new(ssl, socket).unwrap();
        drop(stream);
        drop(peer);
    }
}
