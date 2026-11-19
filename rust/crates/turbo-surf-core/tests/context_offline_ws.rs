use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_core::network_conditions::NetworkConditions;
use turbo_surf_core::websocket::{self, WsCommand, WsEvent, WsRequest};

#[tokio::test(flavor = "current_thread")]
async fn established_websocket_holds_both_directions_until_context_reconnects() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket", listener.local_addr().unwrap());
    let (send_frame, mut frames) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (written, mut writes) = tokio::sync::mpsc::unbounded_channel();
    let (received, mut reads) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            socket.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        let key = head
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-key"))
                    .map(|(_, value)| value.trim())
            })
            .unwrap();
        socket.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",websocket::accept_key(key)).as_bytes()).await.unwrap();
        let (mut read, mut write) = socket.into_split();
        loop {
            let mut bytes = [0; 128];
            tokio::select! {
                Some(frame)=frames.recv()=>{write.write_all(&frame).await.unwrap();written.send(()).unwrap();},
                result=read.read(&mut bytes)=>{match result{Ok(0)|Err(_)=>break,Ok(_)=>{let _=received.send(());}}}
            }
        }
    });
    let conditions = NetworkConditions::default();
    let (handle, mut events) = websocket::open_with_network_conditions(
        WsRequest {
            url: url.clone(),
            ..Default::default()
        },
        conditions.clone(),
    );
    assert!(matches!(events.recv().await, Some(WsEvent::Open(_))));
    conditions.set_offline(true);
    send_frame
        .send([vec![0x81, 4], b"held".to_vec()].concat())
        .unwrap();
    handle.send(WsCommand::Text("client-held".into()));
    writes.recv().await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), reads.recv())
            .await
            .is_err()
    );
    let (blocked, mut failures) = websocket::open_with_network_conditions(
        WsRequest {
            url,
            ..Default::default()
        },
        conditions.clone(),
    );
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), failures.recv())
            .await
            .unwrap(),
        Some(WsEvent::Failed)
    );
    drop(blocked);
    conditions.set_offline(false);
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
            .await
            .unwrap()
        {
            Some(WsEvent::Sent(_)) => continue,
            message => {
                assert_eq!(message, Some(WsEvent::Text("held".into())));
                break;
            }
        }
    }
    tokio::time::timeout(std::time::Duration::from_secs(1), reads.recv())
        .await
        .unwrap()
        .unwrap();
    drop(handle);
    server.abort();
}
