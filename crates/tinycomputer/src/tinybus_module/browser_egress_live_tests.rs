//! Opt-in real-Chromium coverage through the loaded TinyComputer module.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use tinybus::{Connection, broker::Broker, module::ModuleHost, transport::memory::MemoryBus};
use tinycomputer_bus::{
    DesktopResponse,
    browser::names as browser_names,
    browser_egress::{
        self, BrowserEgressConsumeRequest, BrowserEgressDecision, BrowserEgressGrant,
        BrowserEgressTarget, BrowserEgressValidateRequest,
    },
    names,
};

#[derive(Clone)]
struct EgressHost {
    allowed: Arc<AtomicBool>,
    allowed_address: String,
    observations: Arc<Mutex<Vec<String>>>,
}

#[tinybus::interface(name = "ai.tinyhumans.tinycomputer.BrowserEgressHost")]
impl EgressHost {
    async fn authorize(&self, request: BrowserEgressTarget) -> tinybus::Result<BrowserEgressGrant> {
        let allow = self.allowed.load(Ordering::SeqCst)
            && request.hostname == "fixture.test"
            && request.port
                == self
                    .allowed_address
                    .rsplit(':')
                    .next()
                    .and_then(|port| port.parse::<u16>().ok())
                    .unwrap_or_default();
        self.observations.lock().unwrap().push(format!(
            "authorize:{}:{}:{}:{}",
            request.policy_id, request.expected_generation, request.hostname, request.port
        ));
        Ok(BrowserEgressGrant {
            generation: request.expected_generation,
            authorization: if allow {
                "fixture-lease"
            } else {
                "denied-after-policy-change"
            }
            .into(),
            addresses: if allow {
                vec![self.allowed_address.clone()]
            } else {
                Vec::new()
            },
        })
    }

    async fn consume(
        &self,
        request: BrowserEgressConsumeRequest,
    ) -> tinybus::Result<BrowserEgressDecision> {
        self.observations
            .lock()
            .unwrap()
            .push(format!("consume:{}", request.address));
        Ok(BrowserEgressDecision {
            allowed: self.allowed.load(Ordering::SeqCst) && request.address == self.allowed_address,
        })
    }

    async fn validate(
        &self,
        request: BrowserEgressValidateRequest,
    ) -> tinybus::Result<BrowserEgressDecision> {
        self.observations
            .lock()
            .unwrap()
            .push(format!("validate:{}", request.authorization));
        Ok(BrowserEgressDecision {
            allowed: self.allowed.load(Ordering::SeqCst)
                && request.authorization == "fixture-lease",
        })
    }
}

async fn serve_fixture(listener: tokio::net::TcpListener, expected: &'static [u8]) {
    let (mut stream, _) = listener
        .accept()
        .await
        .expect("authorized Chromium socket arrives");
    let mut request = [0_u8; 4096];
    let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut request).await;
    assert!(String::from_utf8_lossy(&request).contains("GET /"));
    let response = [
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: ".as_slice(),
        expected.len().to_string().as_bytes(),
        b"\r\nConnection: close\r\n\r\n".as_slice(),
        expected,
    ]
    .concat();
    let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, &response).await;
}

fn successful_data(reply: &DesktopResponse) -> &serde_json::Value {
    assert!(reply.ok, "{} failed: {:?}", reply.command, reply.error);
    reply.data.as_ref().expect("successful response has data")
}

#[tokio::test]
async fn live_chromium_module_dials_only_the_host_returned_socket_and_rejects_changed_policy()
-> tinybus::Result<()> {
    let allowed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = allowed_listener.local_addr()?.port();
    let denied_listener = tokio::net::TcpListener::bind(("127.0.0.2", port)).await?;
    let allowed_address = allowed_listener.local_addr()?.to_string();
    let allowed_server = tokio::spawn(serve_fixture(allowed_listener, b"allowed fixture"));

    let bus = MemoryBus::new();
    let broker = Broker::new();
    let broker_task = broker.spawn(bus.clone());
    let host_connection = Connection::connect(bus.connect().await?).await?;
    let allowed = Arc::new(AtomicBool::new(true));
    let observations = Arc::new(Mutex::new(Vec::new()));
    host_connection
        .serve_at(
            browser_egress::OBJECT_PATH.try_into()?,
            EgressHost {
                allowed: allowed.clone(),
                allowed_address: allowed_address.clone(),
                observations: observations.clone(),
            },
        )
        .await?;
    host_connection
        .request_name(browser_egress::INTERFACE)
        .await?;

    let module_host = ModuleHost::new(broker);
    let module = crate::linked_module().expect("linked module manifest is valid");
    module_host
        .attach_linked_with_config(
            module,
            serde_json::json!({
                "browser": {"egress": {"policy_id": "fixture-policy", "generation": 41}}
            }),
        )
        .expect("TinyComputer module loads through ModuleHost");
    let client = Connection::connect(bus.connect().await?).await?;
    let proxy = client.proxy(names::INTERFACE, names::OBJECT_PATH, names::INTERFACE)?;

    let opened: DesktopResponse = proxy
        .call(
            browser_names::methods::OPEN_SESSION,
            (serde_json::json!({}),),
        )
        .await?;
    let session = successful_data(&opened)["id"].clone();
    assert_eq!(successful_data(&opened)["launched"], true);
    let url = format!("http://fixture.test:{port}/allowed");
    let navigated: DesktopResponse = proxy
        .call(
            browser_names::methods::NAVIGATE,
            (serde_json::json!({"session": session, "url": url}),),
        )
        .await?;
    assert!(
        navigated.ok,
        "authorized navigation failed: {:?}",
        navigated.error
    );
    tokio::time::timeout(Duration::from_secs(10), allowed_server)
        .await
        .expect("authorized fixture server completes within ten seconds")
        .expect("fixture server task completes");

    let authorizations_before_policy_change = observations
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call.starts_with("authorize:fixture-policy:41:fixture.test:"))
        .count();
    // Simulate a policy refresh: the host changes from granting the fixture
    // socket to denying requests for the same hostname.
    allowed.store(false, Ordering::SeqCst);
    let denied_url = format!("http://fixture.test:{port}/after-policy-change");
    let denied: DesktopResponse = proxy
        .call(
            browser_names::methods::NAVIGATE,
            (serde_json::json!({"session": session, "url": denied_url}),),
        )
        .await?;
    assert!(
        !denied.ok,
        "navigation after host policy denial unexpectedly succeeded"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(500), denied_listener.accept())
            .await
            .is_err(),
        "denied policy-change socket received a connection"
    );

    let calls = observations.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|call| call == &format!("consume:{allowed_address}")),
        "host did not consume the exact returned socket: {calls:?}"
    );
    let authorizations_after_policy_change = calls
        .iter()
        .filter(|call| call.starts_with("authorize:fixture-policy:41:fixture.test:"))
        .count();
    assert!(
        authorizations_after_policy_change > authorizations_before_policy_change,
        "the post-change request did not reach the host authorizer: {calls:?}"
    );
    let denied_address = denied_listener.local_addr()?.to_string();
    assert!(
        !calls
            .iter()
            .any(|call| call == &format!("consume:{denied_address}")),
        "denied socket was consumed: {calls:?}"
    );
    broker_task.abort();
    Ok(())
}
