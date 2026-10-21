//! Fixed-length and chunked HTTP upload forwarding tests.

use super::simulator::*;
use crate::EgressProxy;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn approved_http_request_body_is_forwarded_without_framing_changes() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 512];
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
            if let Some(split) = request.windows(4).position(|window| window == b"\r\n\r\n")
                && request.len() >= split + 8
            {
                break;
            }
        }
        assert!(request.ends_with(b"data"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: true,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "body-session".into(),
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
                "POST http://body.test:{}/submit HTTP/1.1\r\nHost: body.test:{}\r\nProxy-Authorization: {}\r\nContent-Length: 4\r\n\r\ndata",
                address.port(),
                address.port(),
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
            .ends_with("\r\n\r\nok")
    );
    server.await.unwrap();
}

#[tokio::test]
async fn approved_http_request_streams_bodies_larger_than_one_mibibyte() {
    const BODY_LENGTH: usize = 1536 * 1024;
    let expected_body = vec![b'x'; BODY_LENGTH];
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 8192];
        let split = loop {
            let n = socket.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&chunk[..n]);
            if let Some(split) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break split + 4;
            }
        };
        let mut body = request.split_off(split);
        let received = body.len();
        assert!(received <= BODY_LENGTH);
        body.resize(BODY_LENGTH, 0);
        if received < BODY_LENGTH {
            socket.read_exact(&mut body[received..]).await.unwrap();
        }
        assert_eq!(body, vec![b'x'; BODY_LENGTH]);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: true,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "large-body-session".into(),
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
                "POST http://body.test:{}/upload HTTP/1.1\r\nHost: body.test:{}\r\nProxy-Authorization: {}\r\nContent-Length: {BODY_LENGTH}\r\n\r\n",
                address.port(),
                address.port(),
                proxy_authorization()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    client.write_all(&expected_body).await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(
        std::str::from_utf8(&response)
            .unwrap()
            .ends_with("\r\n\r\nok")
    );
    server.await.unwrap();
}

#[tokio::test]
async fn approved_http_request_forwards_chunked_uploads() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&chunk[..n]);
            if request.windows(5).any(|window| window == b"0\r\n\r\n") {
                break;
            }
        }
        assert!(request.windows(7).any(|window| window == b"4\r\ndata"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: true,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "chunked-body-session".into(),
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
                "POST http://body.test:{}/stream HTTP/1.1\r\nHost: body.test:{}\r\nProxy-Authorization: {}\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ndata\r\n0\r\n\r\n",
                address.port(),
                address.port(),
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
            .ends_with("\r\n\r\nok")
    );
    server.await.unwrap();
}

#[tokio::test]
async fn an_early_upstream_rejection_is_forwarded_before_upload_completion() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let n = socket.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&chunk[..n]);
        }
        socket
            .write_all(
                b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
    });
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: true,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "early-rejection-session".into(),
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
                "POST http://body.test:{}/upload HTTP/1.1\r\nHost: body.test:{}\r\nProxy-Authorization: {}\r\nContent-Length: 10485760\r\n\r\n",
                address.port(),
                address.port(),
                proxy_authorization()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.read_to_end(&mut response),
    )
    .await
    .expect("the early upstream response should reach the browser")
    .unwrap();
    assert!(
        std::str::from_utf8(&response)
            .unwrap()
            .starts_with("HTTP/1.1 413")
    );
    server.await.unwrap();
}

#[tokio::test]
async fn a_response_chunk_survives_upload_completion_while_the_client_is_backpressured() {
    const REQUEST_BODY_LENGTH: usize = 1024 * 1024;
    const RESPONSE_BODY_LENGTH: usize = 8 * 1024 * 1024;
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let n = socket.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&chunk[..n]);
        }
        let (mut reader, mut writer) = socket.into_split();
        let request_reader = tokio::spawn(async move {
            let mut body = Vec::new();
            reader.read_to_end(&mut body).await.unwrap();
            body
        });
        writer
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {RESPONSE_BODY_LENGTH}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let body = vec![b'x'; RESPONSE_BODY_LENGTH];
        writer.write_all(&body).await.unwrap();
        writer.shutdown().await.unwrap();
        request_reader.await.unwrap()
    });
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: true,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "response-backpressure-session".into(),
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
                "POST http://body.test:{}/upload HTTP/1.1\r\nHost: body.test:{}\r\nProxy-Authorization: {}\r\nContent-Length: {REQUEST_BODY_LENGTH}\r\n\r\n",
                address.port(),
                address.port(),
                proxy_authorization()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let chunk = [b'y'; 16 * 1024];
    for _ in 0..REQUEST_BODY_LENGTH / chunk.len() {
        client.write_all(&chunk).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    let response_header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("upstream response headers should be present")
        + 4;
    assert_eq!(response.len() - response_header_end, RESPONSE_BODY_LENGTH);
    assert!(
        response[response_header_end..]
            .iter()
            .all(|byte| *byte == b'x')
    );
    assert_eq!(server.await.unwrap().len(), REQUEST_BODY_LENGTH);
}
