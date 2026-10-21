//! Tests for the linked engine that need no browser.
//!
//! agent-browser launches a browser before running any command not on its
//! skip list, so these send only commands that are on it (such as an empty
//! action): the engine answers in its envelope and nothing is launched.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
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
    net::TcpListener,
};

use super::{AgentBrowser, EnforcedAgentBrowser, Linked, enforce_launch, state_options};
use crate::{
    BrowserDialTarget, DialGrant, EgressAuthorizer,
    engine::{Engine, Launcher},
};
use agent_browser::DaemonState;

#[derive(Debug)]
struct RejectAll;

impl EgressAuthorizer for RejectAll {
    fn authorize(
        &self,
        _: BrowserDialTarget,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<DialGrant>> + Send>> {
        Box::pin(async { Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")) })
    }
    fn consume(
        &self,
        _: BrowserDialTarget,
        _: DialGrant,
        _: SocketAddr,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<()>> + Send>> {
        Box::pin(async { Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")) })
    }
    fn validate(
        &self,
        _: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<bool>> + Send>> {
        Box::pin(async { Ok(false) })
    }
}

#[tokio::test]
async fn an_empty_command_is_answered_without_launching_a_browser() {
    let mut engine = AgentBrowser.open("unit");
    let reply = engine.execute(json!({"id": "1", "action": ""})).await;
    assert_eq!(reply["success"], false);
    assert!(
        reply["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn enforced_browser_refuses_implicit_launch_before_any_request() {
    let mut engine = EnforcedAgentBrowser::new(Arc::new(RejectAll)).open("unit");
    let reply = engine
        .execute(json!({"id":"navigate","action":"navigate","url":"http://127.0.0.1/"}))
        .await;
    assert_eq!(reply["success"], false);
    assert!(reply["error"].as_str().unwrap().contains("enforced launch"));
}

#[tokio::test]
async fn a_closed_or_lost_browser_cannot_implicitly_launch_without_the_proxy() {
    let mut engine = Linked {
        state: DaemonState::with_options(state_options("closed")),
        egress_authorizer: Some(Arc::new(RejectAll)),
        proxy: None,
        proxy_session_id: "private-session-id".into(),
        enforced_launch: true,
    };
    let reply = engine
        .execute(json!({"id":"navigate","action":"navigate","url":"http://public.example/"}))
        .await;
    assert_eq!(reply["success"], false);
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("live browser session")
    );
}

#[tokio::test]
#[cfg(feature = "live-tests")]
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
#[cfg(feature = "live-tests")]
async fn live_chromium_subresource_never_reaches_a_denied_destination() {
    let denied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    denied.set_nonblocking(true).unwrap();
    let denied_address = denied.local_addr().unwrap();
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
#[cfg(feature = "live-tests")]
async fn live_chromium_redirect_never_reaches_a_denied_destination() {
    let denied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    denied.set_nonblocking(true).unwrap();
    let denied_address = denied.local_addr().unwrap();
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
#[cfg(feature = "live-tests")]
async fn live_chromium_websocket_never_reaches_a_denied_destination() {
    let denied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    denied.set_nonblocking(true).unwrap();
    let denied_address = denied.local_addr().unwrap();
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

#[derive(Debug)]
struct RejectAllButFixture {
    address: SocketAddr,
    authorizations: Arc<AtomicUsize>,
}

impl EgressAuthorizer for RejectAllButFixture {
    fn authorize(
        &self,
        target: BrowserDialTarget,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<DialGrant>> + Send>> {
        let address = self.address;
        self.authorizations.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if target.hostname == "127.0.0.1" && target.port == address.port() {
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

#[test]
fn enforced_launch_uses_only_the_host_proxy_and_network_containment_flags() {
    let launch = enforce_launch(
        json!({"id":"launch","action":"launch","headless":true,"args":[]}),
        "http://127.0.0.1:39123",
        "session-secret",
    )
    .expect("enforced launch");
    assert_eq!(launch["proxy"]["server"], "http://127.0.0.1:39123");
    assert_eq!(launch["proxy"]["bypass"], "<-loopback>");
    assert_eq!(launch["proxy"]["username"], "tinycomputer");
    assert_eq!(launch["proxy"]["password"], "session-secret");
    assert_eq!(
        launch["args"],
        json!([
            "--disable-quic",
            "--disable-http2",
            "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1",
            "--force-webrtc-ip-handling-policy=disable_non_proxied_udp"
        ])
    );
}

#[test]
fn enforced_launch_refuses_external_cdp_and_caller_flags() {
    let cdp = enforce_launch(
        json!({"action":"launch","cdpUrl":"http://127.0.0.1:9222"}),
        "http://127.0.0.1:39123",
        "session-secret",
    );
    assert!(cdp.is_err());
    let args = enforce_launch(
        json!({"action":"launch","args":["--proxy-server=http://attacker.test"]}),
        "http://127.0.0.1:39123",
        "session-secret",
    );
    assert!(args.is_err());
    let executable = enforce_launch(
        json!({"action":"launch","executablePath":"/tmp/attacker-browser"}),
        "http://127.0.0.1:39123",
        "session-secret",
    );
    assert!(executable.is_err());
}
