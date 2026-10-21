//! Local socket tests for browser proxy enforcement.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use base64::Engine as _;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{BrowserDialTarget, DialGrant, EgressAuthorizer, EgressProxy, egress::parse_authority};

const FIXTURE_PASSWORD: &str = "fixture-proxy-secret";

fn proxy_authorization() -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD
            .encode(format!("tinycomputer:{FIXTURE_PASSWORD}"))
    )
}

#[test]
fn browser_dial_target_debug_does_not_disclose_session_or_destination() {
    let target = BrowserDialTarget {
        scheme: "https".into(),
        hostname: "private.example.test".into(),
        port: 443,
        session_id: "private-browser-session".into(),
    };
    let debug = format!("{target:?}");
    assert!(!debug.contains("private.example.test"));
    assert!(!debug.contains("private-browser-session"));
    assert!(debug.contains("REDACTED"));
}

#[test]
fn dial_grant_debug_does_not_disclose_lease_or_destination() {
    let grant = DialGrant {
        lease: "private-opaque-nonce".into(),
        addresses: vec!["203.0.113.77:8443".parse().unwrap()],
    };
    let debug = format!("{grant:?}");
    assert!(!debug.contains("private-opaque-nonce"));
    assert!(!debug.contains("203.0.113.77"));
    assert!(debug.contains("REDACTED"));
}

#[test]
fn proxy_authority_uses_scheme_defaults_and_canonical_ipv6_literals() {
    assert_eq!(
        parse_authority("example.test", "http", "session")
            .unwrap()
            .port,
        80
    );
    assert_eq!(
        parse_authority("example.test", "https", "session")
            .unwrap()
            .port,
        443
    );
    let ipv6 = parse_authority("[2001:4860:4860::8888]:443", "https", "session").unwrap();
    assert_eq!(ipv6.hostname, "2001:4860:4860::8888");
    assert!(parse_authority("https://example.test:443", "https", "session").is_err());
}

#[derive(Clone)]
struct FixtureAuthorizer {
    address: SocketAddr,
    allow: bool,
    consumed: Arc<AtomicBool>,
}

impl EgressAuthorizer for FixtureAuthorizer {
    fn authorize(
        &self,
        _target: BrowserDialTarget,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<DialGrant>> + Send>> {
        let address = self.address;
        let allow = self.allow;
        Box::pin(async move {
            if allow {
                Ok(DialGrant {
                    lease: "fixture-lease".into(),
                    addresses: vec![address],
                })
            } else {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            }
        })
    }
    fn consume(
        &self,
        _target: BrowserDialTarget,
        _grant: DialGrant,
        address: SocketAddr,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<()>> + Send>> {
        let expected = self.address;
        let consumed = self.consumed.clone();
        Box::pin(async move {
            if address == expected {
                consumed.store(true, Ordering::SeqCst);
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "wrong address",
                ))
            }
        })
    }
    fn validate(
        &self,
        _lease: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<bool>> + Send>> {
        Box::pin(async { Ok(true) })
    }
}

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

#[tokio::test]
async fn proxy_tries_later_addresses_when_the_first_granted_address_is_unreachable() {
    // Port zero is never a listening destination, unlike a recently freed
    // ephemeral port that another parallel test could claim before we dial.
    let first_address = SocketAddr::from(([127, 0, 0, 1], 0));

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
    let proxy = EgressProxy::bind(
        authorizer,
        "multi-address-session".into(),
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
            .ends_with("\r\n\r\nok")
    );
    server.await.unwrap();
}

struct MultiAddressAuthorizer {
    addresses: Vec<SocketAddr>,
}

impl EgressAuthorizer for MultiAddressAuthorizer {
    fn authorize(
        &self,
        _target: BrowserDialTarget,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<DialGrant>> + Send>> {
        let addresses = self.addresses.clone();
        Box::pin(async move {
            Ok(DialGrant {
                lease: "multi-address-lease".into(),
                addresses,
            })
        })
    }

    fn consume(
        &self,
        _target: BrowserDialTarget,
        grant: DialGrant,
        address: SocketAddr,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<()>> + Send>> {
        Box::pin(async move {
            if grant.addresses.contains(&address) {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "wrong address",
                ))
            }
        })
    }

    fn validate(
        &self,
        _lease: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<bool>> + Send>> {
        Box::pin(async { Ok(true) })
    }
}

#[tokio::test]
async fn denied_destination_never_connects_to_the_fixture() {
    let destination = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let address = destination.local_addr().unwrap();
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: false,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "denied-session".into(),
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
                "CONNECT metadata.test:{} HTTP/1.1\r\nHost: metadata.test:{}\r\nProxy-Authorization: {}\r\n\r\n",
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
            .contains("403 Forbidden")
    );
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn unauthenticated_local_clients_cannot_use_the_session_policy_broker() {
    let destination = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let address = destination.local_addr().unwrap();
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: true,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "protected-session".into(),
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
                "CONNECT public.example:{} HTTP/1.1\r\nHost: public.example:{}\r\n\r\n",
                address.port(),
                address.port()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = [0; 256];
    let n = client.read(&mut response).await.unwrap();
    let response = std::str::from_utf8(&response[..n]).unwrap();
    assert!(response.contains("407 Proxy Authentication Required"));
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn approved_http_request_is_rewritten_and_forwarded_to_approved_fixture() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 512];
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = std::str::from_utf8(&request).unwrap();
        assert!(request.starts_with("GET /control?ready=1 HTTP/1.1\r\n"));
        assert!(request.contains(&format!("Host: control.test:{}\r\n", address.port())));
        assert!(request.contains("Connection: close\r\n"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });
    let authorizer = Arc::new(FixtureAuthorizer {
        address,
        allow: true,
        consumed: Arc::new(AtomicBool::new(false)),
    });
    let proxy = EgressProxy::bind(
        authorizer.clone(),
        "http-session".into(),
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
                "GET http://control.test:{}/control?ready=1 HTTP/1.1\r\nHost: control.test:{}\r\nProxy-Authorization: {}\r\nConnection: keep-alive\r\n\r\n",
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
    assert!(authorizer.consumed.load(Ordering::SeqCst));
    server.await.unwrap();
}

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
async fn duplicate_lengths_and_pipelined_authorities_are_refused_before_dial() {
    let destination = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let address = destination.local_addr().unwrap();
    for suffix in [
        "Content-Length: 0\r\nContent-Length: 0\r\n\r\n",
        "Content-Length: 0\r\n\r\nGET http://other.test/ HTTP/1.1\r\nHost: other.test\r\n\r\n",
    ] {
        let proxy = EgressProxy::bind(
            Arc::new(FixtureAuthorizer {
                address,
                allow: true,
                consumed: Arc::new(AtomicBool::new(false)),
            }),
            "malformed-session".into(),
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
                    "GET http://one.test:{}/ HTTP/1.1\r\nHost: one.test:{}\r\nProxy-Authorization: {}\r\n{}",
                    address.port(),
                    address.port(),
                    proxy_authorization(),
                    suffix
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
                .contains("403 Forbidden")
        );
        assert_eq!(
            destination.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
}

#[tokio::test]
async fn approved_websocket_upgrade_is_forwarded_and_tunneled() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = destination.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 512];
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = std::str::from_utf8(&request).unwrap();
        assert!(request.starts_with("GET /socket HTTP/1.1\r\n"));
        assert!(request.contains("Connection: Upgrade\r\n"));
        assert!(request.contains("Upgrade: websocket\r\n"));
        socket
            .write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n")
            .await
            .unwrap();
        let mut ping = [0; 4];
        socket.read_exact(&mut ping).await.unwrap();
        assert_eq!(&ping, b"ping");
        socket.write_all(b"pong").await.unwrap();
    });
    let proxy = EgressProxy::bind(
        Arc::new(FixtureAuthorizer {
            address,
            allow: true,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
        "websocket-session".into(),
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
                "GET ws://socket.test:{}/socket HTTP/1.1\r\nHost: socket.test:{}\r\nProxy-Authorization: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
                address.port(),
                address.port(),
                proxy_authorization()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut handshake = [0; 256];
    let n = client.read(&mut handshake).await.unwrap();
    assert!(
        std::str::from_utf8(&handshake[..n])
            .unwrap()
            .contains("101 Switching Protocols")
    );
    client.write_all(b"ping").await.unwrap();
    let mut pong = [0; 4];
    client.read_exact(&mut pong).await.unwrap();
    assert_eq!(&pong, b"pong");
    server.await.unwrap();
}

struct RevokingAuthorizer {
    address: SocketAddr,
    validations: Arc<std::sync::atomic::AtomicUsize>,
}

impl EgressAuthorizer for RevokingAuthorizer {
    fn authorize(
        &self,
        _target: BrowserDialTarget,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<DialGrant>> + Send>> {
        let address = self.address;
        Box::pin(async move {
            Ok(DialGrant {
                lease: "revoked-during-stream".into(),
                addresses: vec![address],
            })
        })
    }

    fn consume(
        &self,
        _target: BrowserDialTarget,
        _grant: DialGrant,
        address: SocketAddr,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<()>> + Send>> {
        let expected = self.address;
        Box::pin(async move {
            if address == expected {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "wrong address",
                ))
            }
        })
    }

    fn validate(
        &self,
        _lease: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<bool>> + Send>> {
        let validations = self.validations.clone();
        Box::pin(async move {
            let call = validations.fetch_add(1, Ordering::SeqCst);
            Ok(call == 0)
        })
    }
}

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
    tokio::time::timeout(
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
