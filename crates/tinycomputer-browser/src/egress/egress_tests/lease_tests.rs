//! Lease validation and revocation tests for active streams.

use super::simulator::*;
use crate::EgressProxy;
use std::sync::{Arc, atomic::Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn continuous_http_response_still_revalidates_and_closes_revoked_lease() {
    use std::sync::atomic::AtomicUsize;
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = [0; 2048];
        let _ = socket.read(&mut request).await.unwrap();
        for _ in 0..4096 {
            if socket.write_all(&[b'x'; 1024]).await.is_err() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    });
    let validations = Arc::new(AtomicUsize::new(0));
    let proxy = EgressProxy::bind(
        Arc::new(RevokingAuthorizer {
            address,
            validations: validations.clone(),
        }),
        "stream-session".into(),
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
                "GET http://stream.test:{}/ HTTP/1.1\r\nHost: stream.test:{}\r\nProxy-Authorization: {}\r\n\r\n",
                address.port(),
                address.port(),
                proxy_authorization()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(4),
        client.read_to_end(&mut response),
    )
    .await
    .expect("revoked response stream should be closed promptly")
    .unwrap();
    assert!(validations.load(Ordering::SeqCst) > 0);
    assert!(
        response.len() < 4096 * 1024,
        "revoked response kept streaming"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn a_revoked_lease_closes_a_large_request_upload() {
    use std::sync::atomic::AtomicUsize;
    const BODY_LENGTH: usize = 10 * 1024 * 1024;
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut received = Vec::new();
        socket.read_to_end(&mut received).await.unwrap();
        received
    });
    let validations = Arc::new(AtomicUsize::new(0));
    let proxy = EgressProxy::bind(
        Arc::new(RevokingAuthorizer {
            address,
            validations: validations.clone(),
        }),
        "upload-revocation-session".into(),
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
                "POST http://upload.test:{}/ HTTP/1.1\r\nHost: upload.test:{}\r\nProxy-Authorization: {}\r\nContent-Length: {BODY_LENGTH}\r\n\r\n",
                address.port(),
                address.port(),
                proxy_authorization()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let chunk = [b'x'; 1024];
    for _ in 0..BODY_LENGTH / chunk.len() {
        if client.write_all(&chunk).await.is_err() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let mut response = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(4),
        client.read_to_end(&mut response),
    )
    .await
    .expect("revoked upload should be closed promptly");
    let received = server.await.unwrap();
    assert!(validations.load(Ordering::SeqCst) > 1);
    assert!(received.len() < BODY_LENGTH);
}
