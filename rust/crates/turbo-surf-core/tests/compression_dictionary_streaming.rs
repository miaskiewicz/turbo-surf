//! Genuine HTTPS headers/body backpressure and dictionary-codec lifecycle controls.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Notify, Semaphore},
};
use turbo_surf_core::{
    http_cache::{CacheMode, HttpCache},
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
struct Server {
    url: String,
    first: Arc<Semaphore>,
    last: Arc<Semaphore>,
    closed: Arc<Notify>,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
    hints: ClientHints,
    expected: Vec<u8>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn permission(
    socket: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    gate: &Semaphore,
    closed: &Notify,
) -> bool {
    let mut byte = [0];
    tokio::select! {
        permit=gate.acquire()=>{permit.unwrap().forget();true}
        read=socket.read(&mut byte)=>{assert!(!matches!(read, Ok(count) if count != 0));closed.notify_one();false}
    }
}
impl Server {
    async fn new(encoding: &'static str, truncate: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://{}", listener.local_addr().unwrap());
        let first = Arc::new(Semaphore::new(0));
        let last = Arc::new(Semaphore::new(0));
        let closed = Arc::new(Notify::new());
        let requests = Arc::new(AtomicUsize::new(0));
        let mut encoded = if encoding == "dcb" {
            DCB.to_vec()
        } else {
            DCZ.to_vec()
        };
        let split = if encoding == "dcb" {
            encoded.len() - 1
        } else {
            let split = encoded.len();
            encoded.extend_from_within(40..);
            split
        };
        let mut expected = DECODED.to_vec();
        if encoding == "dcz" {
            expected.extend_from_slice(DECODED);
        }
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
        let (first_gate, last_gate, closed_event, count) = (
            first.clone(),
            last.clone(),
            closed.clone(),
            requests.clone(),
        );
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let (acceptor, first, last, closed, count, encoded) = (
                    acceptor.clone(),
                    first_gate.clone(),
                    last_gate.clone(),
                    closed_event.clone(),
                    count.clone(),
                    encoded.clone(),
                );
                children.spawn(async move {
                    let Ok(mut socket)=acceptor.accept(socket).await else{return;};
                    let mut head=Vec::new();loop {let mut byte=[0];if socket.read(&mut byte).await.unwrap_or(0)==0{return;}head.extend_from_slice(&byte);if head.ends_with(b"\r\n\r\n"){break;}}
                    let text=String::from_utf8(head).unwrap();let path=text.split_whitespace().nth(1).unwrap();
                    if path=="/dictionary" {
                        let header=format!("HTTP/1.1 200 OK\r\nConnection: close\r\nCache-Control: max-age=60\r\nUse-As-Dictionary: match=\"/resource/*\"\r\nContent-Length: {}\r\n\r\n",DICTIONARY.len());
                        socket.write_all(header.as_bytes()).await.unwrap();socket.write_all(DICTIONARY).await.unwrap();let _=socket.shutdown().await;return;
                    }
                    count.fetch_add(1,Ordering::SeqCst);
                    let header=format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: text/html\r\nCache-Control: max-age=60\r\nUse-As-Dictionary: match=\"/child/*\"\r\nContent-Encoding: {encoding}\r\nVary: Available-Dictionary, Accept-Encoding\r\nContent-Length: {}\r\n\r\n",encoded.len());
                    socket.write_all(header.as_bytes()).await.unwrap();socket.flush().await.unwrap();
                    if !permission(&mut socket,&first,&closed).await{return;}
                    if socket.write_all(&encoded[..split]).await.is_err(){return;}
                    socket.flush().await.unwrap();
                    if !permission(&mut socket,&last,&closed).await{return;}
                    if !truncate{let _=socket.write_all(&encoded[split..]).await;}
                    let _=socket.shutdown().await;
                });
                while children.try_join_next().is_some() {}
            }
        });
        Self {
            url,
            first,
            last,
            closed,
            requests,
            task,
            hints: ClientHints::with_client_builder(|| net::client_builder_with_trust(CERT)),
            expected,
        }
    }
    fn options<'a>(&'a self, cache: &'a HttpCache) -> FetchOptions<'a> {
        FetchOptions {
            http_cache: Some(cache),
            client_hints: Some(&self.hints),
            cache_partition: "stream-partition".into(),
            context: Some(RequestContext::resource(
                &format!("{}/page", self.url),
                Destination::Empty,
                RequestMode::Cors,
            )),
            allow_non_html: true,
            ..Default::default()
        }
    }
    async fn seed(&self, cache: &HttpCache) {
        assert_eq!(
            net::fetch_bytes(&format!("{}/dictionary", self.url), self.options(cache))
                .await
                .unwrap()
                .1,
            DICTIONARY
        );
    }
    fn child_registered(&self, cache: &HttpCache) -> bool {
        cache
            .dictionary_store()
            .select(
                "stream-partition",
                &format!("{}/child/result", self.url),
                &self.options(cache).context.unwrap(),
            )
            .is_some()
    }
    async fn assert_not_cached(&self, cache: &HttpCache) {
        let mut options = self.options(cache);
        options.cache_mode = CacheMode::OnlyIfCached;
        assert!(
            net::fetch_bytes(&format!("{}/resource/stream", self.url), options)
                .await
                .unwrap_err()
                .message
                .contains("cache miss")
        );
        assert!(!self.child_registered(cache));
    }
}
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(30), future)
        .await
        .expect("gated transport must progress without releasing its withheld phase")
}
#[tokio::test]
async fn dictionary_headers_and_decoded_prefix_arrive_before_withheld_encoded_tail() {
    for encoding in ["dcb", "dcz"] {
        let server = Server::new(encoding, false).await;
        let cache = HttpCache::default();
        server.seed(&cache).await;
        let mut response = bounded(net::fetch_stream(
            &format!("{}/resource/stream", server.url),
            server.options(&cache),
        ))
        .await
        .unwrap();
        assert!(!server.child_registered(&cache));
        server.first.add_permits(1);
        let first = bounded(response.body.next_chunk()).await.unwrap().unwrap();
        assert_eq!(&first[..], &server.expected[..first.len()]);
        assert!(!server.child_registered(&cache));
        server.last.add_permits(1);
        let mut body = first;
        while let Some(chunk) = bounded(response.body.next_chunk()).await.unwrap() {
            body.extend_from_slice(&chunk);
        }
        assert_eq!(body, server.expected);
        assert!(server.child_registered(&cache));
        assert_eq!(
            net::fetch_bytes(
                &format!("{}/resource/stream", server.url),
                server.options(&cache)
            )
            .await
            .unwrap()
            .1,
            server.expected
        );
        assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn cancelling_dictionary_response_after_headers_or_prefix_closes_transport_without_publication(
) {
    for encoding in ["dcb", "dcz"] {
        for partial in [false, true] {
            let server = Server::new(encoding, false).await;
            let cache = HttpCache::default();
            server.seed(&cache).await;
            let mut response = bounded(net::fetch_stream(
                &format!("{}/resource/stream", server.url),
                server.options(&cache),
            ))
            .await
            .unwrap();
            if partial {
                server.first.add_permits(1);
                assert!(!bounded(response.body.next_chunk())
                    .await
                    .unwrap()
                    .unwrap()
                    .is_empty());
            }
            response.body.cancel();
            bounded(server.closed.notified()).await;
            server.assert_not_cached(&cache).await;
        }
    }
}
#[tokio::test]
async fn incomplete_or_output_capped_dictionary_stream_never_commits_decoded_cache_or_dictionary() {
    for encoding in ["dcb", "dcz"] {
        for truncate in [false, true] {
            let server = Server::new(encoding, truncate).await;
            let cache = HttpCache::default();
            server.seed(&cache).await;
            let mut options = server.options(&cache);
            if !truncate {
                options.max_bytes = Some(1024);
            }
            let mut response = bounded(net::fetch_stream(
                &format!("{}/resource/stream", server.url),
                options,
            ))
            .await
            .unwrap();
            server.first.add_permits(1);
            server.last.add_permits(1);
            let error = loop {
                match bounded(response.body.next_chunk()).await {
                    Ok(Some(_)) => {}
                    Ok(None) => panic!("incomplete or oversized stream cannot succeed"),
                    Err(error) => break error,
                }
            };
            if !truncate {
                assert_eq!(error.code, net::ErrorCode::BodyTooLarge);
            }
            server.assert_not_cached(&cache).await;
        }
    }
}
