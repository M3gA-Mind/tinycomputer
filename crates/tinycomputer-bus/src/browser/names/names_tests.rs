//! Tests for the browser members' bus identity.
//!
//! These pin strings a host spells from this crate and a module answers to. A
//! change to one of them is a wire break, so it should have to be made twice —
//! once in the constant and once here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{INTERFACE, METHODS, OBJECT_PATH, methods};

#[test]
fn browser_members_share_the_module_interface() {
    // Literals, not the aliases they are defined from: a change to the
    // published identity must be made here on purpose.
    assert_eq!(INTERFACE, "ai.tinyhumans.tinycomputer.Desktop");
    assert_eq!(OBJECT_PATH, "/ai/tinyhumans/tinycomputer/Desktop");
}

#[test]
fn methods_lists_every_member_once() {
    let mut sorted = METHODS.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    sorted.dedup();

    assert_eq!(sorted.len(), count, "METHODS contains a duplicate");
    assert_eq!(count, 15);
}

#[test]
fn every_browser_member_carries_the_browser_prefix() {
    for member in METHODS {
        assert!(member.starts_with("Browser"), "{member} lacks the prefix");
        assert!(
            member.chars().all(|c| c.is_ascii_alphanumeric()),
            "{member} is not a bare identifier"
        );
    }
}

#[test]
fn browser_members_are_served_on_the_module_interface() {
    for member in METHODS {
        assert!(
            crate::names::METHODS.contains(member),
            "{member} is missing from the module's member list"
        );
    }
}

#[test]
fn every_member_constant_appears_in_methods() {
    for member in [
        methods::OPEN_SESSION,
        methods::CLOSE_SESSION,
        methods::LIST_SESSIONS,
        methods::NAVIGATE,
        methods::SNAPSHOT,
        methods::PERFORM,
        methods::READ_PAGE,
        methods::EVALUATE,
        methods::SCREENSHOT,
        methods::READ_OUTPUT,
        methods::RELEASE_OUTPUT,
        methods::LIST_DOWNLOADS,
        methods::WAIT_DOWNLOAD,
        methods::CLAIM_PROFILE,
        methods::RELEASE_PROFILE,
    ] {
        assert!(
            METHODS.contains(&member),
            "{member} is missing from METHODS"
        );
    }
}
