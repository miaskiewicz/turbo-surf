//! A one-shot, bounded upload stream. The renderer owns the producer, never a realm.
#[cfg(feature = "impersonate")]
use crate::net::ErrorCode;
use crate::net::HttpError;
pub use crate::upload::{UploadBody, UploadSender};

impl UploadBody {
    pub(crate) fn into_body(self) -> Result<crate::http_backend::Body, HttpError> {
        Ok(crate::http_backend::Body::wrap_stream(self.take_stream()?))
    }
}

/// Upload pools admit only a normally negotiated HTTP/2 connection. Applying a
/// request version on wreq would also alter ALPN; inspect the connection instead.
#[cfg(feature = "impersonate")]
pub(crate) fn guard(
    builder: crate::http_backend::ClientBuilder,
) -> crate::http_backend::ClientBuilder {
    builder.connector_layer(tower::layer::layer_fn(Guard))
}
#[cfg(not(feature = "impersonate"))]
pub(crate) fn guard(
    builder: crate::http_backend::ClientBuilder,
) -> crate::http_backend::ClientBuilder {
    builder
}
#[cfg(feature = "impersonate")]
#[derive(Clone)]
struct Guard<S>(S);
#[cfg(feature = "impersonate")]
impl<S, T> tower::Service<T> for Guard<S>
where
    S: tower::Service<T, Error = Box<dyn std::error::Error + Send + Sync>>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Response: wreq::connector::Connection + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;
    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }
    fn call(&mut self, request: T) -> Self::Future {
        use wreq::connector::Connection;
        let future = self.0.call(request);
        Box::pin(async move {
            let connection = future.await?;
            if !connection.connected().is_negotiated_h2() {
                return Err(Box::new(HttpError {
                    message: "Streaming requests require HTTP/2".into(),
                    code: ErrorCode::Network,
                }) as Self::Error);
            }
            Ok(connection)
        })
    }
}

#[cfg(all(test, feature = "impersonate"))]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    // Observe the actual ClientHello before rejecting the handshake. Randoms,
    // GREASE values and extension order may vary, but admission must not change ALPN.
    async fn offered_alpn(
        builder: crate::http_backend::ClientBuilder,
        label: &str,
    ) -> Vec<Vec<u8>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = builder
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let capture = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = [0_u8; 5];
            socket.read_exact(&mut header).await.unwrap();
            assert_eq!(header[0], 22);
            let length = usize::from(u16::from_be_bytes([header[3], header[4]]));
            let mut hello = vec![0; length];
            socket.read_exact(&mut hello).await.unwrap();
            if let Some(directory) = std::env::var_os("TURBO_SURF_TRANSPORT_CAPTURE_DIR") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let mut record = header.to_vec();
                record.extend_from_slice(&hello);
                std::fs::write(directory.join(format!("{label}-client-hello.bin")), record)
                    .unwrap();
            }
            assert_eq!(hello[0], 1);
            let mut at = 4 + 2 + 32;
            at += 1 + usize::from(hello[at]);
            let cipher_bytes = usize::from(u16::from_be_bytes([hello[at], hello[at + 1]]));
            at += 2 + cipher_bytes;
            at += 1 + usize::from(hello[at]);
            let extension_bytes = usize::from(u16::from_be_bytes([hello[at], hello[at + 1]]));
            at += 2;
            let end = at + extension_bytes;
            assert!(end <= hello.len());
            while at < end {
                let kind = u16::from_be_bytes([hello[at], hello[at + 1]]);
                let size = usize::from(u16::from_be_bytes([hello[at + 2], hello[at + 3]]));
                at += 4;
                if kind == 16 {
                    let mut protocols = Vec::new();
                    let stop = at + size;
                    at += 2;
                    while at < stop {
                        let size = usize::from(hello[at]);
                        at += 1;
                        protocols.push(hello[at..at + size].to_vec());
                        at += size;
                    }
                    return protocols;
                }
                at += size;
            }
            panic!("ClientHello omitted ALPN")
        };
        let request = client.get(format!("https://{address}/")).send();
        let (protocols, response) =
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(capture, request)
            })
            .await
            .unwrap();
        assert!(response.is_err());
        protocols
    }

    #[tokio::test]
    async fn h2_upload_admission_preserves_the_normal_chrome_alpn_offer() {
        let normal = offered_alpn(crate::net::client_builder(), "native-normal").await;
        let upload = offered_alpn(guard(crate::net::client_builder()), "native-upload").await;
        assert_eq!(normal, vec![b"h2".to_vec(), b"http/1.1".to_vec()]);
        assert_eq!(upload, normal);
    }
}
