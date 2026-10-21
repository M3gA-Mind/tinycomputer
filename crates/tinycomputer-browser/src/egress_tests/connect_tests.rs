//! CONNECT proxy tests.

use super::simulator::*;
use crate::EgressProxy;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}};

#[tokio::test]
async fn approved_connect_reaches_only_the_consumed_socket() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let authorizer = Arc::new(FixtureAuthorizer {
        address,
        allow: true,
        consumed: Arc::new(AtomicBool::new(false)),
    });
    let proxy = EgressProxy::bind(
        authorizer.clone(),
        "test-session".into(),
        FIXTURE_PASSWORD.into(),
    )
    .await
    .unwrap();
    let ((), upstream_client) = tokio::join!(
        async {
            let mut client = TcpStream::connect(proxy.url().replace("http://", ""))
                .await
                .unwrap();
            client
                .write_all(
                    format!(
                        "CONNECT example.test:{} HTTP/1.1\r\nHost: example.test:{}\r\nProxy-Authorization: {}\r\n\r\n",
                        address.port(),
                        address.port(),
                        proxy_authorization()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut response = [0; 128];
            let n = client.read(&mut response).await.unwrap();
            assert!(
                std::str::from_utf8(&response[..n])
                    .unwrap()
                    .contains("200 Connection Established")
            );
            client.write_all(b"ping").await.unwrap();
        },
        destination.accept()
    );
    let (mut upstream_client, _) = upstream_client.unwrap();
    assert!(authorizer.consumed.load(Ordering::SeqCst));
    let mut payload = [0; 4];
    upstream_client.read_exact(&mut payload).await.unwrap();
    assert_eq!(&payload, b"ping");
}
