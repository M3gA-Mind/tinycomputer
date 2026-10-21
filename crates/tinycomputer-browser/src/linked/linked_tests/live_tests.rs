//! Live Chromium egress tests that use local HTTP and WebSocket fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::EnforcedAgentBrowser;
use crate::engine::Launcher;
use crate::{BrowserDialTarget, DialGrant, EgressAuthorizer};
use serde_json::json;
use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
#[cfg(live_browser_tests)]
async fn live_chromium_loopback_navigation_uses_the_authenticated_session_proxy() {
    let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = fixture.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = fixture.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 22\r\nConnection: close\r\n\r\n<h1>proxy fixture</h1>",
            )
            .await
            .unwrap();
    });
    let authorizations = Arc::new(AtomicUsize::new(0));
    let mut browser = EnforcedAgentBrowser::new(Arc::new(RejectAllButFixture {
        address,
        authorizations: authorizations.clone(),
        denied_targets: Arc::new(Mutex::new(Vec::new())),
    }))
    .open("live-egress");
    let launch = browser
        .execute(json!({"id":"launch","action":"launch","headless":true,"args":[]}))
        .await;
    assert_eq!(launch["success"], true, "{launch}");
    let navigation = browser
        .execute(json!({
            "id":"navigate",
            "action":"navigate",
            "url":format!("http://127.0.0.1:{}/", address.port())
        }))
        .await;
    browser
        .execute(json!({"id":"close","action":"close"}))
        .await;
    assert_eq!(
        navigation["success"],
        true,
        "{navigation}; proxy authorizations: {}",
        authorizations.load(Ordering::SeqCst)
    );
    server.await.unwrap();
}

#[tokio::test]
#[cfg(live_browser_tests)]
async fn live_chromium_subresource_never_reaches_a_denied_destination() {
    let denied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    denied.set_nonblocking(true).unwrap();
    let denied_address = denied.local_addr().unwrap();
    let denied_targets = Arc::new(Mutex::new(Vec::new()));
    let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = fixture.local_addr().unwrap();
    let body = format!(
        "<img src=\"http://127.0.0.1:{}/private.png\">",
        denied_address.port()
    );
    let fixture_server = tokio::spawn(async move {
        let (mut socket, _) = fixture.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    let mut browser = EnforcedAgentBrowser::new(Arc::new(RejectAllButFixture {
        address,
        authorizations: Arc::new(AtomicUsize::new(0)),
        denied_targets: denied_targets.clone(),
    }))
    .open("live-egress-subresource");
    let launch = browser
        .execute(json!({"id":"launch","action":"launch","headless":true,"args":[]}))
        .await;
    assert_eq!(launch["success"], true, "{launch}");
    let navigation = browser
        .execute(json!({
            "id":"navigate",
            "action":"navigate",
            "url":format!("http://127.0.0.1:{}/", address.port())
        }))
        .await;
    wait_for_denied_target(&denied_targets, denied_address.port()).await;
    browser
        .execute(json!({"id":"close","action":"close"}))
        .await;
    assert_eq!(navigation["success"], true, "{navigation}");
    fixture_server.await.unwrap();
    assert_eq!(
        denied.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock,
        "the denied subresource destination must never receive a connection"
    );
}

#[tokio::test]
#[cfg(live_browser_tests)]
async fn live_chromium_redirect_never_reaches_a_denied_destination() {
    let denied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    denied.set_nonblocking(true).unwrap();
    let denied_address = denied.local_addr().unwrap();
    let denied_targets = Arc::new(Mutex::new(Vec::new()));
    let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = fixture.local_addr().unwrap();
    let fixture_server = tokio::spawn(async move {
        let (mut socket, _) = fixture.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{}/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    denied_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    let mut browser = EnforcedAgentBrowser::new(Arc::new(RejectAllButFixture {
        address,
        authorizations: Arc::new(AtomicUsize::new(0)),
        denied_targets: denied_targets.clone(),
    }))
    .open("live-egress-redirect");
    let launch = browser
        .execute(json!({"id":"launch","action":"launch","headless":true,"args":[]}))
        .await;
    assert_eq!(launch["success"], true, "{launch}");
    let navigation = browser
        .execute(json!({
            "id":"navigate",
            "action":"navigate",
            "url":format!("http://127.0.0.1:{}/", address.port())
        }))
        .await;
    wait_for_denied_target(&denied_targets, denied_address.port()).await;
    browser
        .execute(json!({"id":"close","action":"close"}))
        .await;
    assert_eq!(navigation["success"], false, "{navigation}");
    fixture_server.await.unwrap();
    assert_eq!(
        denied.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock,
        "the denied redirect destination must never receive a connection"
    );
}

#[tokio::test]
#[cfg(live_browser_tests)]
async fn live_chromium_websocket_never_reaches_a_denied_destination() {
    let denied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    denied.set_nonblocking(true).unwrap();
    let denied_address = denied.local_addr().unwrap();
    let denied_targets = Arc::new(Mutex::new(Vec::new()));
    let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = fixture.local_addr().unwrap();
    let body = format!(
        "<script>new WebSocket('ws://127.0.0.1:{}/private')</script>",
        denied_address.port()
    );
    let fixture_server = tokio::spawn(async move {
        let (mut socket, _) = fixture.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    let mut browser = EnforcedAgentBrowser::new(Arc::new(RejectAllButFixture {
        address,
        authorizations: Arc::new(AtomicUsize::new(0)),
        denied_targets: denied_targets.clone(),
    }))
    .open("live-egress-websocket");
    let launch = browser
        .execute(json!({"id":"launch","action":"launch","headless":true,"args":[]}))
        .await;
    assert_eq!(launch["success"], true, "{launch}");
    let navigation = browser
        .execute(json!({
            "id":"navigate",
            "action":"navigate",
            "url":format!("http://127.0.0.1:{}/", address.port())
        }))
        .await;
    wait_for_denied_target(&denied_targets, denied_address.port()).await;
    browser
        .execute(json!({"id":"close","action":"close"}))
        .await;
    assert_eq!(navigation["success"], true, "{navigation}");
    fixture_server.await.unwrap();
    assert_eq!(
        denied.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock,
        "the denied WebSocket destination must never receive a connection"
    );
}

#[cfg(live_browser_tests)]
#[derive(Debug)]
struct RejectAllButFixture {
    address: SocketAddr,
    authorizations: Arc<AtomicUsize>,
    denied_targets: Arc<Mutex<Vec<(String, u16)>>>,
}

#[cfg(live_browser_tests)]
async fn wait_for_denied_target(denied_targets: &Arc<Mutex<Vec<(String, u16)>>>, port: u16) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let was_denied = denied_targets
                .lock()
                .unwrap()
                .iter()
                .any(|(_, denied_port)| *denied_port == port);
            if was_denied {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Chromium should attempt the denied destination");
}

#[cfg(live_browser_tests)]
impl EgressAuthorizer for RejectAllButFixture {
    fn authorize(
        &self,
        target: BrowserDialTarget,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<DialGrant>> + Send>> {
        let address = self.address;
        self.authorizations.fetch_add(1, Ordering::SeqCst);
        let allowed = target.hostname == "127.0.0.1" && target.port == address.port();
        if !allowed {
            self.denied_targets
                .lock()
                .unwrap()
                .push((target.hostname, target.port));
        }
        Box::pin(async move {
            if allowed {
                Ok(DialGrant {
                    lease: "live-fixture-lease".into(),
                    addresses: vec![address],
                })
            } else {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            }
        })
    }

    fn consume(
        &self,
        _: BrowserDialTarget,
        grant: DialGrant,
        address: SocketAddr,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<()>> + Send>> {
        Box::pin(async move {
            if grant.addresses.contains(&address) {
                Ok(())
            } else {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            }
        })
    }

    fn validate(
        &self,
        _: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<bool>> + Send>> {
        Box::pin(async { Ok(true) })
    }
}
