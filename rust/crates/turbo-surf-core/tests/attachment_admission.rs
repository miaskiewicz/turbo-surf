use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::net::{fetch_html, ErrorCode, FetchOptions};

#[tokio::test]
async fn attachment_admission_is_explicit_and_does_not_admit_other_binary_documents() {
    for (disposition, allow, admitted) in [
        ("attachment; filename=owned.bin", false, false),
        ("attachment; filename=owned.bin", true, true),
        (" AtTaChMeNt ; filename=owned.bin", true, true),
        ("inline; filename=owned.bin", true, false),
        ("attachment-other; filename=owned.bin", true, false),
        ("filename=attachment", true, false),
        ("", true, false),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/download", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
            }
            let headers = format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: {disposition}\r\nContent-Length: 4\r\nConnection: close\r\n\r\n");
            socket.write_all(headers.as_bytes()).await.unwrap();
            let _ = socket.write_all(&[0, 255, 65, 10]).await;
        });
        let response = fetch_html(
            &url,
            FetchOptions {
                allow_attachments: allow,
                ..Default::default()
            },
        )
        .await;
        if admitted {
            assert_eq!(response.unwrap().body_bytes, [0, 255, 65, 10]);
        } else {
            assert_eq!(response.unwrap_err().code, ErrorCode::NotHtml);
        }
        server.await.unwrap();
    }
}
