//! Focused browser egress proxy tests.

use super::simulator::*;
use crate::EgressProxy;
use serde_json::json;
use std::{io, net::SocketAddr, sync::*};
use tokio::{io::*, net::*};

#[tokio::test]
async fn approved_websocket_upgrade_is_forwarded_and_tunneled() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 512];
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = std::str::from_utf8(&request).unwrap();
        assert!(request.starts_with("GET /socket HTTP/1.1\r\n"));
        assert!(request.contains("Connection: Upgrade\r\n"));
        assert!(request.contains("Upgrade: websocket\r\n"));
        socket
            .write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n")
            .await
            .unwrap();
        let mut ping = [0; 4];
        socket.read_exact(&mut ping).await.unwrap();
        assert_eq!(&ping, b"ping");
        socket.write_all(b"pong").await.unwrap();
    });
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: true,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "websocket-session".into(),
        FIXTURE_PASSWORD.into(),
    )
    .await
    .unwrap();
    let mut client = TcpStream::connect(proxy.url().replace("http://", ""))
        .await
        .unwrap();
    client
        .write_all(
            format!(
                "GET ws://socket.test:{}/socket HTTP/1.1\r\nHost: socket.test:{}\r\nProxy-Authorization: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
                address.port(),
                address.port(),
                proxy_authorization()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut handshake = [0; 256];
    let n = client.read(&mut handshake).await.unwrap();
    assert!(
        std::str::from_utf8(&handshake[..n])
            .unwrap()
            .contains("101 Switching Protocols")
    );
    client.write_all(b"ping").await.unwrap();
    let mut pong = [0; 4];
    client.read_exact(&mut pong).await.unwrap();
    assert_eq!(&pong, b"pong");
    server.await.unwrap();
}
