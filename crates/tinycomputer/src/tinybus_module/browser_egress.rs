//! Browser network callback adapter. Policy and DNS decisions live in the
//! authenticated host callback, which delegates to `TinySecurity`; this module
//! only forwards requests and enforces the returned exact sockets.

use std::{future::Future, io, net::SocketAddr, pin::Pin};

use tinybus::Connection;
use tinycomputer_browser::{BrowserDialTarget, DialGrant, EgressAuthorizer};
use tinycomputer_bus::browser_egress::{
    self as contract, BrowserEgressConsumeRequest, BrowserEgressTarget,
    BrowserEgressValidateRequest,
};

const CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Authenticated callback adapter to the host's `TinySecurity` bus bridge.
#[derive(Clone)]
pub(super) struct BusAuthorizer {
    connection: Connection,
    policy_id: String,
    generation: u64,
}

impl std::fmt::Debug for BusAuthorizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BusAuthorizer")
            .finish_non_exhaustive()
    }
}

impl BusAuthorizer {
    pub(super) fn new(connection: Connection, policy_id: String, generation: u64) -> Self {
        Self {
            connection,
            policy_id,
            generation,
        }
    }

    fn proxy(&self) -> io::Result<tinybus::Proxy> {
        self.connection
            .proxy(
                contract::INTERFACE,
                contract::OBJECT_PATH,
                contract::INTERFACE,
            )
            .map(|proxy| proxy.with_timeout(CALLBACK_TIMEOUT))
            .map_err(denied)
    }

    fn target(&self, target: BrowserDialTarget) -> BrowserEgressTarget {
        BrowserEgressTarget {
            policy_id: self.policy_id.clone(),
            expected_generation: self.generation,
            session_id: target.session_id,
            scheme: target.scheme,
            hostname: target.hostname,
            port: target.port,
        }
    }
}

impl EgressAuthorizer for BusAuthorizer {
    fn authorize(
        &self,
        target: BrowserDialTarget,
    ) -> Pin<Box<dyn Future<Output = io::Result<DialGrant>> + Send>> {
        let this = self.clone();
        Box::pin(async move {
            let expected_port = target.port;
            let request = this.target(target);
            let response: contract::BrowserEgressGrant = this
                .proxy()?
                .call(contract::methods::AUTHORIZE, (request,))
                .await
                .map_err(denied)?;
            if response.generation != this.generation
                || response.authorization.is_empty()
                || response.addresses.is_empty()
                || response.addresses.len() > 64
            {
                return Err(denied("host returned a stale or empty browser grant"));
            }
            let addresses = response
                .addresses
                .iter()
                .map(|address| address.parse::<SocketAddr>().map_err(denied))
                .collect::<io::Result<Vec<_>>>()?;
            if addresses
                .iter()
                .any(|address| address.port() != expected_port)
            {
                return Err(denied("host returned an address for a different port"));
            }
            Ok(DialGrant {
                lease: response.authorization,
                addresses,
            })
        })
    }

    fn consume(
        &self,
        target: BrowserDialTarget,
        grant: DialGrant,
        address: SocketAddr,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send>> {
        let this = self.clone();
        Box::pin(async move {
            if !grant.addresses.contains(&address) {
                return Err(denied("socket address was not in the host grant"));
            }
            let request = BrowserEgressConsumeRequest {
                target: this.target(target),
                authorization: grant.lease,
                address: address.to_string(),
            };
            let decision: contract::BrowserEgressDecision = this
                .proxy()?
                .call(contract::methods::CONSUME, (request,))
                .await
                .map_err(denied)?;
            if decision.allowed {
                Ok(())
            } else {
                Err(denied("TinySecurity refused browser socket grant"))
            }
        })
    }

    fn validate(&self, lease: String) -> Pin<Box<dyn Future<Output = io::Result<bool>> + Send>> {
        let this = self.clone();
        Box::pin(async move {
            let request = BrowserEgressValidateRequest {
                authorization: lease,
            };
            let decision: contract::BrowserEgressDecision = this
                .proxy()?
                .call(contract::methods::VALIDATE, (request,))
                .await
                .map_err(denied)?;
            Ok(decision.allowed)
        })
    }
}

fn denied(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, error.to_string())
}

#[cfg(test)]
mod browser_egress_tests;
