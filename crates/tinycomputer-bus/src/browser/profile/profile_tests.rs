//! Wire-shape tests for the profile claim types.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::json;

#[test]
fn a_request_without_options_waits_not_and_may_fall_back() {
    let request: ClaimProfileRequest = serde_json::from_value(json!({"path": "/p"})).unwrap();
    assert_eq!(request.wait_ms, 0);
    assert!(request.fall_back_to_fresh);
}

#[test]
fn outcomes_travel_as_snake_case() {
    for (outcome, name) in [
        (ProfileClaim::Claimed, "claimed"),
        (ProfileClaim::Busy, "busy"),
        (ProfileClaim::FellBackToFresh, "fell_back_to_fresh"),
    ] {
        assert_eq!(serde_json::to_value(outcome).unwrap(), json!(name));
    }
}

#[test]
fn a_busy_answer_carries_no_path() {
    let busy = ClaimProfileResponse {
        outcome: ProfileClaim::Busy,
        path: None,
    };
    assert_eq!(
        serde_json::to_value(busy).unwrap(),
        json!({"outcome": "busy"})
    );
}
