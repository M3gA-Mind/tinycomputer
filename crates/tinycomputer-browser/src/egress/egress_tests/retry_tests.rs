//! Proxy retry behavior for multiple approved addresses.

use super::simulator::*;
use crate::{BrowserDialTarget, EgressAuthorizer, egress::connect_approved_addresses};
use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{net::TcpListener, net::TcpStream};

#[tokio::test]
async fn proxy_tries_later_addresses_when_the_first_granted_address_is_unreachable() {
    let first_address = SocketAddr::from(([127, 0, 0, 1], 1));
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = destination.accept().await.unwrap();
        socket.local_addr().unwrap()
    });
    let target = BrowserDialTarget {
        scheme: "http".into(),
        hostname: "control.test".into(),
        port: second_address.port(),
        session_id: "multi-address-session".into(),
    };
    let authorizer: Arc<dyn EgressAuthorizer> = Arc::new(MultiAddressAuthorizer {
        addresses: vec![first_address, second_address],
    });
    let first_grant = authorizer.authorize(target.clone()).await.unwrap();
    let dial_attempts = Arc::new(AtomicUsize::new(0));
    let counted_attempts = Arc::clone(&dial_attempts);
    let mut connect = move |address| {
        counted_attempts.fetch_add(1, Ordering::SeqCst);
        async move {
            if address == first_address {
                Err(io::Error::other("simulated unreachable address"))
            } else {
                TcpStream::connect(address).await
            }
        }
    };

    let (upstream, lease) =
        connect_approved_addresses(&authorizer, &target, first_grant, &mut connect)
            .await
            .unwrap();

    assert_eq!(lease, "multi-address-lease");
    assert_eq!(upstream.peer_addr().unwrap(), second_address);
    assert_eq!(server.await.unwrap(), second_address);
    assert_eq!(dial_attempts.load(Ordering::SeqCst), 2);
}
