//! Focused browser egress proxy tests.

use super::simulator::*;
use crate::EgressProxy;
use serde_json::json;
use std::{io, net::SocketAddr, sync::*};
use tokio::{io::*, net::*};

#[tokio::test]
async fn dropping_session_proxy_closes_its_listener() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address: destination.local_addr().unwrap(),
            allow: false,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "closed-session".into(),
        FIXTURE_PASSWORD.into(),
    )
    .await
    .unwrap();
    let endpoint = proxy.url().replace("http://", "");
    drop(proxy);
    tokio::task::yield_now().await;
    assert!(TcpStream::connect(endpoint).await.is_err());
}
