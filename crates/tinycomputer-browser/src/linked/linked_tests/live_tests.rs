//! Live Chromium egress tests that use local HTTP and WebSocket fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::EnforcedAgentBrowser;
use crate::engine::Launcher;
use crate::{BrowserDialTarget, DialGrant, EgressAuthorizer};
use serde_json::json;
use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
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
async fn live_chromium_uses_a_host_permitted_cross_origin_subresource_socket() {
    let cdn_fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cdn_address = cdn_fixture.local_addr().unwrap();
    let page_fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let page_address = page_fixture.local_addr().unwrap();
    let html = format!(
        "<img src=\"http://cdn.fixture.test:{}/asset.png\"><p>page fixture</p>",
        cdn_address.port()
    );
    let page_server = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), page_fixture.accept())
            .await
            .expect("Chromium requests the permitted page")
            .unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    let cdn_server = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), cdn_fixture.accept())
            .await
            .expect("Chromium requests the permitted cross-origin asset")
            .unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let addresses = HashMap::from([
        ("127.0.0.1".to_owned(), page_address),
        ("cdn.fixture.test".to_owned(), cdn_address),
    ]);
    let mut browser = EnforcedAgentBrowser::new(Arc::new(AllowFixtureHosts {
        addresses,
        authorizations: Arc::new(AtomicUsize::new(0)),
    }))
    .open("live-egress-permitted-cross-origin");
    let launch = browser
        .execute(json!({"id":"launch","action":"launch","headless":true,"args":[]}))
        .await;
    assert_eq!(launch["success"], true, "{launch}");
    let navigation = browser
        .execute(json!({
            "id":"navigate",
            "action":"navigate",
            "url":format!("http://127.0.0.1:{}/", page_address.port())
        }))
        .await;
    browser
        .execute(json!({"id":"close","action":"close"}))
        .await;
    assert_eq!(navigation["success"], true, "{navigation}");
    page_server.await.unwrap();
    cdn_server.await.unwrap();
}

#[tokio::test]
#[cfg(live_browser_tests)]
async fn live_chromium_worker_fetch_never_reaches_a_denied_destination() {
    let denied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    denied.set_nonblocking(true).unwrap();
    let denied_address = denied.local_addr().unwrap();
    let denied_targets = Arc::new(Mutex::new(Vec::new()));
    let fixture = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = fixture.local_addr().unwrap();
    let html = format!("<script>new Worker('/worker.js')</script><p>worker fixture</p>");
    let worker = format!(
        "fetch('http://127.0.0.1:{}/private').catch(() => {{}});",
        denied_address.port()
    );
    let fixture_server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), fixture.accept())
                .await
                .expect("Chromium requests the page and worker script")
                .unwrap();
            let mut request = [0; 4096];
            let size = socket.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            let (status, content_type, body, worker_script) = if request.contains("GET /worker.js")
            {
                ("200 OK", "application/javascript", worker.as_str(), true)
            } else if request.contains("GET / ") {
                ("200 OK", "text/html", html.as_str(), false)
            } else {
                ("404 Not Found", "text/plain", "not found", false)
            };
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            if worker_script {
                break;
            }
        }
    });
    let mut browser = EnforcedAgentBrowser::new(Arc::new(RejectAllButFixture {
        address,
        authorizations: Arc::new(AtomicUsize::new(0)),
        denied_targets: denied_targets.clone(),
    }))
    .open("live-egress-worker");
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
        "the denied worker fetch destination must never receive a connection"
    );
}

#[tokio::test]
#[cfg(live_browser_tests)]
async fn live_chromium_dns_rebinding_never_reaches_the_rebound_private_socket() {
    let allowed_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = allowed_listener.local_addr().unwrap().port();
    let private = std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, port)).unwrap();
    private.set_nonblocking(true).unwrap();
    let private_address = private.local_addr().unwrap();
    let allowed_address = allowed_listener.local_addr().unwrap();
    let extra_allowed_connections = Arc::new(AtomicUsize::new(0));
    let observed_addresses = Arc::new(Mutex::new(Vec::new()));
    let extra_connections = extra_allowed_connections.clone();
    let body = format!("<img src=\"http://fixture.test:{port}/after-rebind.png\"><p>fixture</p>");
    let allowed_server = tokio::spawn(async move {
        let (mut socket, _) =
            tokio::time::timeout(Duration::from_secs(10), allowed_listener.accept())
                .await
                .expect("authorized navigation reaches the allowed fixture")
                .unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        drop(socket);

        while let Ok((socket, _)) = allowed_listener.accept().await {
            extra_connections.fetch_add(1, Ordering::SeqCst);
            drop(socket);
        }
    });
    let denied_targets = Arc::new(Mutex::new(Vec::new()));
    let mut browser = EnforcedAgentBrowser::new(Arc::new(RebindingAuthorizer {
        allowed_address,
        private_address,
        authorizations: AtomicUsize::new(0),
        denied_targets: denied_targets.clone(),
        observed_addresses: observed_addresses.clone(),
    }))
    .open("live-egress-dns-rebinding");
    let launch = browser
        .execute(json!({"id":"launch","action":"launch","headless":true,"args":[]}))
        .await;
    assert_eq!(launch["success"], true, "{launch}");
    let navigation = browser
        .execute(json!({
            "id":"navigate",
            "action":"navigate",
            "url":format!("http://fixture.test:{port}/")
        }))
        .await;
    wait_for_denied_target(&denied_targets, port).await;
    browser
        .execute(json!({"id":"close","action":"close"}))
        .await;
    assert_eq!(navigation["success"], true, "{navigation}");
    let observed_addresses = observed_addresses.lock().unwrap();
    assert_eq!(
        observed_addresses.first(),
        Some(&allowed_address),
        "the controlled resolver first returns the permitted address"
    );
    assert!(
        observed_addresses.len() >= 2
            && observed_addresses[1..]
                .iter()
                .all(|address| *address == private_address),
        "the controlled resolver must rebind the name to the private address: {observed_addresses:?}"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(500), async {
            while extra_allowed_connections.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_err(),
        "the denied rebound request reached the previously allowed socket"
    );
    assert_eq!(
        private.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock,
        "the rebound private destination must never receive a connection"
    );
    allowed_server.abort();
    let _ = allowed_server.await;
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
#[derive(Debug)]
struct RebindingAuthorizer {
    allowed_address: SocketAddr,
    private_address: SocketAddr,
    authorizations: AtomicUsize,
    denied_targets: Arc<Mutex<Vec<(String, u16)>>>,
    observed_addresses: Arc<Mutex<Vec<SocketAddr>>>,
}

#[cfg(live_browser_tests)]
#[derive(Debug)]
struct AllowFixtureHosts {
    addresses: HashMap<String, SocketAddr>,
    authorizations: Arc<AtomicUsize>,
}

#[cfg(live_browser_tests)]
impl EgressAuthorizer for AllowFixtureHosts {
    fn authorize(
        &self,
        target: BrowserDialTarget,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<DialGrant>> + Send>> {
        self.authorizations.fetch_add(1, Ordering::SeqCst);
        let address = self
            .addresses
            .get(&target.hostname)
            .copied()
            .filter(|address| address.port() == target.port);
        Box::pin(async move {
            match address {
                Some(address) => Ok(DialGrant {
                    lease: "live-cross-origin-fixture-lease".into(),
                    addresses: vec![address],
                }),
                None => Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
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

#[cfg(live_browser_tests)]
impl EgressAuthorizer for RebindingAuthorizer {
    fn authorize(
        &self,
        target: BrowserDialTarget,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<DialGrant>> + Send>> {
        let attempt = self.authorizations.fetch_add(1, Ordering::SeqCst);
        let resolved_address = if attempt == 0 {
            self.allowed_address
        } else {
            self.private_address
        };
        self.observed_addresses
            .lock()
            .unwrap()
            .push(resolved_address);
        let allowed = attempt == 0
            && target.hostname == "fixture.test"
            && target.port == self.allowed_address.port();
        if !allowed {
            self.denied_targets
                .lock()
                .unwrap()
                .push((target.hostname, target.port));
        }
        Box::pin(async move {
            if allowed {
                Ok(DialGrant {
                    lease: "live-dns-rebinding-lease".into(),
                    addresses: vec![resolved_address],
                })
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "rebound private address denied",
                ))
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

#[cfg(live_browser_tests)]
async fn wait_for_denied_target(denied_targets: &Arc<Mutex<Vec<(String, u16)>>>, port: u16) {
    tokio::time::timeout(Duration::from_secs(10), async {
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
