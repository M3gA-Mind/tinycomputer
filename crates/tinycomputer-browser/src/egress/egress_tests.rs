//! Listener lifecycle tests for the browser egress proxy.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io;
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn an_accept_error_waits_then_allows_later_connections() {
    let failed = super::accept_connection(std::future::ready(Err(io::Error::other(
        "transient accept failure",
    ))))
    .await;
    assert!(failed.is_none());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = TcpStream::connect(address);
    let (accepted, connected) = tokio::join!(listener.accept(), client);
    let accepted = super::accept_connection(std::future::ready(accepted)).await;

    assert!(accepted.is_some());
    connected.unwrap();
}
