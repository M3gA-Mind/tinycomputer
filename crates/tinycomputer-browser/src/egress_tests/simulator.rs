//! Local authorizers and fixture helpers shared by egress tests.

use crate::{BrowserDialTarget, DialGrant, EgressAuthorizer};
use base64::Engine as _;
use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

pub(super) const FIXTURE_PASSWORD: &str = "fixture-proxy-secret";

pub(super) fn proxy_authorization() -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD
            .encode(format!("tinycomputer:{FIXTURE_PASSWORD}"))
    )
}

#[derive(Clone)]
pub(super) struct FixtureAuthorizer {
    pub(super) address: SocketAddr,
    pub(super) allow: bool,
    pub(super) consumed: Arc<AtomicBool>,
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

pub(super) struct MultiAddressAuthorizer {
    pub(super) addresses: Vec<SocketAddr>,
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

pub(super) struct RevokingAuthorizer {
    pub(super) address: SocketAddr,
    pub(super) validations: Arc<AtomicUsize>,
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
