//! Authored HTTPS wire controls use independently encoded official C fixtures.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::{
    http_cache::HttpCache,
    net::{self, FetchOptions},
    request_context::{ClientHints, Destination, RequestContext, RequestMode},
};

const DICTIONARY: &[u8] =
    include_bytes!("../../../../harness/detection/fixtures/compression-dictionary/dictionary.bin");
const DECODED: &[u8] =
    include_bytes!("../../../../harness/detection/fixtures/compression-dictionary/decoded.bin");
const DCB: &[u8] =
    include_bytes!("../../../../harness/detection/fixtures/compression-dictionary/encoded.dcb");
const DCZ: &[u8] =
    include_bytes!("../../../../harness/detection/fixtures/compression-dictionary/encoded.dcz");
const CERT: &[u8] = include_bytes!("../../turbo-surf-render/tests/csp_report_cert.der");
const KEY: &[u8] = include_bytes!("../../turbo-surf-render/tests/csp_report_key.der");

type RequestRows = Arc<Mutex<Vec<(String, BTreeMap<String, String>)>>>;

struct Server {
    url: String,
    rows: RequestRows,
    task: tokio::task::JoinHandle<()>,
    hints: ClientHints,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://{}", listener.local_addr().unwrap());
        let rows = Arc::new(Mutex::new(Vec::new()));
        let output = rows.clone();
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
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                let output = output.clone();
                children.spawn(async move {
                    let Ok(mut socket) = acceptor.accept(socket).await else {
                        return;
                    };
                    let mut request = Vec::new();
                    loop {
                        let mut byte = [0];
                        if socket.read(&mut byte).await.unwrap_or(0) == 0 {
                            return;
                        }
                        request.extend_from_slice(&byte);
                        if request.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    let text = String::from_utf8(request).unwrap();
                    let head = text.starts_with("HEAD ");
                    let path = text.split_whitespace().nth(1).unwrap().to_owned();
                    let headers: BTreeMap<String, String> = text.lines().skip(1)
                        .filter_map(|line| line.split_once(':'))
                        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_owned()))
                        .collect();
                    output.lock().unwrap().push((path.clone(), headers.clone()));
                    let dictionary = path.starts_with("/dictionary");
                    let plain = path == "/clear";
                    let encoding = if path.ends_with("dcz") { "dcz" } else { "dcb" };
                    let bytes = if dictionary { DICTIONARY } else if plain { b"cleared" } else if encoding == "dcz" { DCZ } else { DCB };
                    let mut body = bytes.to_vec();
                    if path.contains("bad-hash") { body[10] ^= 1; }
                    let revalidated = headers.contains_key("if-none-match");
                    let status = if revalidated { "304 Not Modified" } else { "200 OK" };
                    let cache_control = if path.contains("no-store") { "no-store" } else { "max-age=60" };
                    let mut response = format!("HTTP/1.1 {status}\r\nConnection: close\r\nCache-Control: {cache_control}\r\nETag: \"authored\"\r\nContent-Type: text/html\r\n");
                    if plain { response.push_str("Clear-Site-Data: \"cache\"\r\n"); }
                    if dictionary { response.push_str("Use-As-Dictionary: match=\"/resource/*\", id=\"wire-dictionary\"\r\n"); }
                    else if !plain { response.push_str(&format!("Content-Encoding: {encoding}\r\nVary: Available-Dictionary, Accept-Encoding\r\n")); }
                    if path.contains("repeated-encoding") {
                        response.push_str("Content-Encoding: gzip\r\n");
                    }
                    let length = if path.contains("large-head") { usize::MAX } else if revalidated { 0 } else { body.len() };
                    response.push_str(&format!("Content-Length: {length}\r\n\r\n"));
                    if socket.write_all(response.as_bytes()).await.is_ok() && !revalidated && !head {
                        let _ = socket.write_all(&body).await;
                    }
                    let _ = socket.shutdown().await;
                });
                while children.try_join_next().is_some() {}
            }
        });
        Self {
            url,
            rows,
            task,
            hints: ClientHints::with_client_builder(|| net::client_builder_with_trust(CERT)),
        }
    }
    fn options<'a>(&'a self, cache: &'a HttpCache) -> FetchOptions<'a> {
        FetchOptions {
            http_cache: Some(cache),
            client_hints: Some(&self.hints),
            cache_partition: "authored-partition".into(),
            context: Some(RequestContext::resource(
                &format!("{}/page", self.url),
                Destination::Empty,
                RequestMode::Cors,
            )),
            allow_non_html: true,
            ..Default::default()
        }
    }
}

#[tokio::test]
async fn official_dcb_dcz_decode_actual_wire_and_cached_bytes_without_double_decoding() {
    let server = Server::new().await;
    let cache = HttpCache::default();
    let options = server.options(&cache);
    assert_eq!(
        net::fetch_bytes(&format!("{}/dictionary", server.url), options)
            .await
            .unwrap()
            .1,
        DICTIONARY
    );
    // The HTTP representation remains cached after dictionary-sidecar eviction.
    // Replaying it must republish the complete dictionary without another request.
    cache.dictionary_store().clear();
    assert_eq!(
        net::fetch_bytes(
            &format!("{}/dictionary", server.url),
            server.options(&cache)
        )
        .await
        .unwrap()
        .1,
        DICTIONARY
    );
    for encoding in ["dcb", "dcz"] {
        let url = format!("{}/resource/{encoding}", server.url);
        for _ in 0..2 {
            let options = server.options(&cache);
            assert_eq!(net::fetch_bytes(&url, options).await.unwrap().1, DECODED);
        }
        let options = server.options(&cache);
        let mut result = net::fetch_stream(&url, options).await.unwrap();
        assert!(result
            .headers
            .iter()
            .any(|(name, value)| name == "content-encoding" && value == encoding));
        let mut body = Vec::new();
        while let Some(chunk) = result.body.next_chunk().await.unwrap() {
            body.extend_from_slice(&chunk);
        }
        assert_eq!(body, DECODED);
        let mut options = server.options(&cache);
        options.cache_mode = turbo_surf_core::http_cache::CacheMode::NoCache;
        assert_eq!(net::fetch_bytes(&url, options).await.unwrap().1, DECODED);
    }
    let rows = server.rows.lock().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|(path, _)| path == "/dictionary")
            .count(),
        1
    );
    for (path, headers) in rows
        .iter()
        .filter(|(path, _)| path.starts_with("/resource/"))
    {
        assert_eq!(
            headers.get("accept-encoding").map(String::as_str),
            Some("gzip, deflate, br, zstd, dcb, dcz"),
            "{path}"
        );
        assert_eq!(
            headers.get("dictionary-id").map(String::as_str),
            Some("\"wire-dictionary\"")
        );
        assert!(headers.contains_key("available-dictionary"));
    }
    assert_eq!(
        rows.iter()
            .filter(|(path, _)| path == "/resource/dcb")
            .count(),
        2
    );
    assert_eq!(
        rows.iter()
            .filter(|(path, _)| path == "/resource/dcz")
            .count(),
        2
    );
}

#[tokio::test]
async fn actual_wire_rejects_missing_advertisement_wrong_partition_hash_and_output_limit() {
    let server = Server::new().await;
    let cache = HttpCache::default();
    let options = server.options(&cache);
    let error = net::fetch_bytes(&format!("{}/resource/dcb", server.url), options)
        .await
        .unwrap_err();
    assert!(error.message.contains("without an advertised dictionary"));
    let options = server.options(&cache);
    net::fetch_bytes(&format!("{}/dictionary", server.url), options)
        .await
        .unwrap();
    let mut options = server.options(&cache);
    options.cache_partition = "other-partition".into();
    assert!(
        net::fetch_bytes(&format!("{}/resource/dcb", server.url), options)
            .await
            .unwrap_err()
            .message
            .contains("without an advertised dictionary")
    );
    let options = server.options(&cache);
    assert!(
        net::fetch_bytes(&format!("{}/resource/bad-hash", server.url), options)
            .await
            .unwrap_err()
            .message
            .contains("SHA-256 mismatch")
    );
    assert!(net::fetch_bytes(
        &format!("{}/resource/repeated-encoding", server.url),
        server.options(&cache),
    )
    .await
    .unwrap_err()
    .message
    .contains("Unsupported dictionary encoding chain"));
    let mut options = server.options(&cache);
    options.max_bytes = Some(DECODED.len() - 1);
    assert_eq!(
        net::fetch_bytes(&format!("{}/resource/dcz", server.url), options)
            .await
            .unwrap_err()
            .code,
        net::ErrorCode::BodyTooLarge
    );
}

#[tokio::test]
async fn incomplete_and_cancelled_dictionary_bodies_never_publish_or_cache_partial_bytes() {
    let server = Server::new().await;
    let cache = HttpCache::default();
    let mut first = net::fetch_stream(
        &format!("{}/dictionary", server.url),
        server.options(&cache),
    )
    .await
    .unwrap();
    assert!(first.body.next_chunk().await.unwrap().is_some());
    first.body.cancel();
    assert!(cache
        .dictionary_store()
        .select(
            "authored-partition",
            &format!("{}/resource/dcb", server.url),
            server.options(&cache).context.as_ref().unwrap()
        )
        .is_none());
    assert_eq!(
        net::fetch_bytes(
            &format!("{}/dictionary", server.url),
            server.options(&cache)
        )
        .await
        .unwrap()
        .1,
        DICTIONARY
    );
    assert_eq!(
        net::fetch_bytes(
            &format!("{}/resource/dcb", server.url),
            server.options(&cache)
        )
        .await
        .unwrap()
        .1,
        DECODED
    );
    assert_eq!(
        server
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/dictionary")
            .count(),
        2
    );
}

#[tokio::test]
async fn clear_site_data_invalidates_dictionary_and_preclear_inflight_body_commit() {
    let server = Server::new().await;
    let cache = HttpCache::default();
    let mut pending = net::fetch_stream(
        &format!("{}/dictionary", server.url),
        server.options(&cache),
    )
    .await
    .unwrap();
    assert_eq!(
        net::fetch_bytes(&format!("{}/clear", server.url), server.options(&cache))
            .await
            .unwrap()
            .1,
        b"cleared"
    );
    while pending.body.next_chunk().await.unwrap().is_some() {}
    assert!(net::fetch_bytes(
        &format!("{}/resource/dcb", server.url),
        server.options(&cache)
    )
    .await
    .unwrap_err()
    .message
    .contains("without an advertised dictionary"));
    net::fetch_bytes(
        &format!("{}/dictionary", server.url),
        server.options(&cache),
    )
    .await
    .unwrap();
    assert_eq!(
        net::fetch_bytes(
            &format!("{}/resource/dcz", server.url),
            server.options(&cache)
        )
        .await
        .unwrap()
        .1,
        DECODED
    );
    // Reload the clear response so its side effect is genuine network traffic.
    let mut options = server.options(&cache);
    options.cache_mode = turbo_surf_core::http_cache::CacheMode::Reload;
    net::fetch_bytes(&format!("{}/clear", server.url), options)
        .await
        .unwrap();
    assert!(net::fetch_bytes(
        &format!("{}/resource/dcb", server.url),
        server.options(&cache)
    )
    .await
    .unwrap_err()
    .message
    .contains("without an advertised dictionary"));
}

#[tokio::test]
async fn no_store_and_denied_response_policies_never_register_dictionaries() {
    let server = Server::new().await;
    let cache = HttpCache::default();
    net::fetch_bytes(
        &format!("{}/dictionary-no-store", server.url),
        server.options(&cache),
    )
    .await
    .unwrap();
    let denied = |_: &RequestContext, _: &str, _: &[(String, String)]| false;
    let mut options = server.options(&cache);
    options.response_policy = Some(&denied);
    assert!(
        net::fetch_bytes(&format!("{}/dictionary", server.url), options)
            .await
            .unwrap_err()
            .message
            .contains("policy")
    );
    assert!(net::fetch_bytes(
        &format!("{}/resource/dcb", server.url),
        server.options(&cache)
    )
    .await
    .unwrap_err()
    .message
    .contains("without an advertised dictionary"));
}

#[tokio::test]
async fn head_metadata_never_registers_a_dictionary_without_its_representation_body() {
    let server = Server::new().await;
    let cache = HttpCache::default();
    let mut options = server.options(&cache);
    options.method = Some("HEAD".into());
    assert_eq!(
        net::fetch_bytes(&format!("{}/dictionary-head", server.url), options)
            .await
            .unwrap()
            .1,
        Vec::<u8>::new()
    );
    assert!(cache
        .dictionary_store()
        .select(
            "authored-partition",
            &format!("{}/resource/dcb", server.url),
            &server.options(&cache).context.unwrap()
        )
        .is_none());
    assert!(net::fetch_bytes(
        &format!("{}/resource/dcb", server.url),
        server.options(&cache)
    )
    .await
    .unwrap_err()
    .message
    .contains("without an advertised dictionary"));
    {
        let rows = server.rows.lock().unwrap();
        let (_, headers) = rows
            .iter()
            .find(|(path, _)| path == "/resource/dcb")
            .unwrap();
        assert_eq!(headers.get("available-dictionary"), None);
        assert!(!headers
            .get("accept-encoding")
            .is_some_and(|value| value.contains("dcb")));
    }
    assert_eq!(
        net::fetch_bytes(
            &format!("{}/dictionary-get", server.url),
            server.options(&cache)
        )
        .await
        .unwrap()
        .1,
        DICTIONARY
    );
    assert_eq!(
        net::fetch_bytes(
            &format!("{}/resource/dcb", server.url),
            server.options(&cache)
        )
        .await
        .unwrap()
        .1,
        DECODED
    );
}

#[tokio::test]
async fn head_encoded_resources_preserve_metadata_without_decoding_or_body_cap_errors() {
    let server = Server::new().await;
    let cache = HttpCache::default();
    net::fetch_bytes(
        &format!("{}/dictionary", server.url),
        server.options(&cache),
    )
    .await
    .unwrap();
    for encoding in ["dcb", "dcz"] {
        let mut options = server.options(&cache);
        options.method = Some("HEAD".into());
        options.max_bytes = Some(0);
        let mut result = net::fetch_stream(
            &format!("{}/resource/large-head-{encoding}", server.url),
            options,
        )
        .await
        .unwrap();
        assert!(result
            .headers
            .iter()
            .any(|(name, value)| name == "content-encoding" && value == encoding));
        assert!(result
            .headers
            .iter()
            .any(|(name, value)| name == "content-length" && value == &usize::MAX.to_string()));
        assert_eq!(result.body.next_chunk().await.unwrap(), None);
        let mut options = server.options(&cache);
        options.method = Some("HEAD".into());
        options.max_bytes = Some(0);
        assert_eq!(
            net::fetch_bytes(
                &format!("{}/resource/large-head-{encoding}", server.url),
                options
            )
            .await
            .unwrap()
            .1,
            Vec::<u8>::new()
        );
    }
    let client = net::client_builder_with_trust(CERT)
        .unwrap()
        .build()
        .unwrap();
    let options = FetchOptions {
        client: Some(&client),
        method: Some("HEAD".into()),
        max_bytes: Some(0),
        ..Default::default()
    };
    assert_eq!(
        net::fetch_bytes(&format!("{}/resource/large-head-dcb", server.url), options)
            .await
            .unwrap()
            .1,
        Vec::<u8>::new()
    );
}
