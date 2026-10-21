//! Proxy retry behavior for multiple approved addresses.

use super::simulator::*;
use crate::EgressProxy;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn proxy_tries_later_addresses_when_the_first_granted_address_is_unreachable() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_address = destination.local_addr().unwrap();
    // Port 1 on the primary loopback interface is closed on the test runners,
    // so the refusal is immediate and cannot race another ephemeral listener.
    let first_address = SocketAddr::from(([127, 0, 0, 1], 1));
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });
    let authorizer = Arc::new(MultiAddressAuthorizer {
        addresses: vec![first_address, second_address],
    });
    let proxy = EgressProxy::bind(
        authorizer,
        "multi-address-session".into(),
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
                "GET http://control.test:{}/ HTTP/1.1\r\nHost: control.test:{}\r\nProxy-Authorization: {}\r\n\r\n",
                second_address.port(),
                second_address.port(),
                proxy_authorization()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(
        std::str::from_utf8(&response)
            .unwrap()
            .ends_with("\r\n\r\nok"),
        "unexpected proxy response: {}",
        String::from_utf8_lossy(&response)
    );
    server.await.unwrap();
}
