//! Proxy retry behavior for multiple approved addresses.

use super::simulator::*;
use crate::egress::serve_with_connector;
use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn proxy_tries_later_addresses_when_the_first_granted_address_is_unreachable() {
    let first_address = SocketAddr::from(([127, 0, 0, 1], 1));
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_address = destination.local_addr().unwrap();
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
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = proxy_listener.local_addr().unwrap();
    let (client, proxy_connection) =
        tokio::join!(TcpStream::connect(proxy_address), proxy_listener.accept());
    let mut client = client.unwrap();
    let (proxy_stream, _) = proxy_connection.unwrap();
    let dial_attempts = Arc::new(AtomicUsize::new(0));
    let counted_attempts = Arc::clone(&dial_attempts);
    let service = tokio::spawn(serve_with_connector(
        proxy_stream,
        authorizer,
        "multi-address-session".into(),
        FIXTURE_PASSWORD.into(),
        move |address| {
            counted_attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if address == first_address {
                    Err(io::Error::other("simulated unreachable address"))
                } else {
                    TcpStream::connect(address).await
                }
            }
        },
    ));
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
    let _service_result = service.await.unwrap();
    assert_eq!(dial_attempts.load(Ordering::SeqCst), 2);
}
