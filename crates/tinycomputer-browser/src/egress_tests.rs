//! Egress proxy tests, grouped by protocol path and lifecycle behavior.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "egress_tests/authority_tests.rs"]
mod authority_tests;
#[path = "egress_tests/connect_tests.rs"]
mod connect_tests;
#[path = "egress_tests/http_tests.rs"]
mod http_tests;
#[path = "egress_tests/lease_tests.rs"]
mod lease_tests;
#[path = "egress_tests/lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "egress_tests/retry_tests.rs"]
mod retry_tests;
#[path = "egress_tests/simulator.rs"]
mod simulator;
#[path = "egress_tests/websocket_tests.rs"]
mod websocket_tests;
