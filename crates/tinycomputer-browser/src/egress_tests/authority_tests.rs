//! Focused browser egress proxy tests.

use super::simulator::*;
use crate::{BrowserDialTarget, DialGrant, EgressProxy, egress::parse_authority};
use serde_json::json;
use std::{io, net::*, sync::*};
use tokio::{io::*, net::*};

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
