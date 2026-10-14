//! Tests for the interaction payload types.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{Action, ActionOutcome, LocateBy, Locator, ScrollDirection, Target, WaitState};
use crate::browser::PageState;
use serde_json::json;

#[test]
fn parse_reads_an_at_prefix_as_a_ref() {
    assert_eq!(Target::parse("@e12"), Target::reference("e12"));
    assert_eq!(Target::parse("  @e12  "), Target::reference("e12"));
}

#[test]
fn parse_reads_anything_else_as_a_selector() {
    assert_eq!(Target::parse("#submit"), Target::selector("#submit"));
    assert_eq!(
        Target::parse("button[type=submit]"),
        Target::selector("button[type=submit]")
    );
}

#[test]
fn reference_drops_a_leading_at_however_it_arrives() {
    assert_eq!(Target::reference("@e3"), Target::reference("e3"));
}

#[test]
fn target_is_tagged_by_kind_on_the_wire() {
    assert_eq!(
        serde_json::to_value(Target::reference("e2")).expect("serializes"),
        json!({ "kind": "ref", "value": "e2" })
    );
    assert_eq!(
        serde_json::to_value(Target::selector("#a")).expect("serializes"),
        json!({ "kind": "selector", "value": "#a" })
    );
}

#[test]
fn locator_round_trips_through_its_wire_form() {
    let target = Target::locator(Locator::new(LocateBy::Role, "button").with_name("Submit"));
    let encoded = serde_json::to_value(&target).expect("serializes");

    assert_eq!(
        encoded,
        json!({
            "kind": "locator",
            "value": {
                "by": "role",
                "value": "button",
                "name": "Submit",
                "exact": false,
                "index": 0,
            },
        })
    );
    assert_eq!(
        serde_json::from_value::<Target>(encoded).expect("deserializes"),
        target
    );
}

#[test]
fn locator_fills_its_optional_fields_from_the_default() {
    let locator: Locator =
        serde_json::from_value(json!({ "by": "test_id", "value": "cart" })).expect("deserializes");

    assert_eq!(locator.by, LocateBy::TestId);
    assert_eq!(locator.index, 0);
    assert!(!locator.exact);
}

#[test]
fn action_is_tagged_by_action_on_the_wire() {
    let click = Action::Click {
        target: Target::reference("e2"),
        new_tab: false,
    };

    assert_eq!(
        serde_json::to_value(&click).expect("serializes"),
        json!({
            "action": "click",
            "target": { "kind": "ref", "value": "e2" },
            "new_tab": false,
        })
    );
    assert_eq!(
        serde_json::from_value::<Action>(json!({
            "action": "click",
            "target": { "kind": "ref", "value": "e2" },
        }))
        .expect("deserializes"),
        click
    );
}

#[test]
fn unit_actions_are_a_bare_tag() {
    assert_eq!(
        serde_json::to_value(Action::Reload).expect("serializes"),
        json!({ "action": "reload" })
    );
    assert_eq!(
        serde_json::from_value::<Action>(json!({ "action": "back" })).expect("deserializes"),
        Action::Back
    );
}

#[test]
fn typing_defaults_to_the_focused_element() {
    let action: Action =
        serde_json::from_value(json!({ "action": "type", "text": "hello" })).expect("deserializes");

    assert_eq!(
        action,
        Action::Type {
            target: None,
            text: "hello".to_string(),
            delay_ms: None,
        }
    );
}

#[test]
fn a_press_carries_its_target_only_when_it_has_one() {
    let untargeted = Action::Press {
        key: "Enter".to_string(),
        target: None,
    };
    assert_eq!(
        serde_json::to_value(&untargeted).expect("serializes"),
        json!({ "action": "press", "key": "Enter" })
    );
    assert_eq!(
        serde_json::from_value::<Action>(json!({ "action": "press", "key": "Enter" }))
            .expect("deserializes"),
        untargeted
    );
    let targeted = Action::Press {
        key: " ".to_string(),
        target: Some(Target::reference("e5")),
    };
    let encoded = json!({
        "action": "press",
        "key": " ",
        "target": { "kind": "ref", "value": "e5" },
    });
    assert_eq!(
        serde_json::to_value(&targeted).expect("serializes"),
        encoded
    );
    assert_eq!(
        serde_json::from_value::<Action>(encoded).expect("deserializes"),
        targeted
    );
}

#[test]
fn wait_for_defaults_to_waiting_for_visibility() {
    let action: Action = serde_json::from_value(json!({
        "action": "wait_for",
        "target": { "kind": "selector", "value": ".ready" },
    }))
    .expect("deserializes");

    let Action::WaitFor { state, .. } = action else {
        panic!("expected a wait_for action");
    };
    assert_eq!(state, WaitState::Visible);
}

#[test]
fn scroll_direction_is_snake_case_on_the_wire() {
    assert_eq!(
        serde_json::to_value(ScrollDirection::Bottom).expect("serializes"),
        json!("bottom")
    );
}

#[test]
fn an_acting_outcome_carries_no_value() {
    let outcome = ActionOutcome::acted(PageState::new("https://example.com/"));

    assert!(outcome.value.is_null());
    assert!(outcome.matched.is_none());
}

#[test]
fn a_reading_outcome_round_trips_with_its_match() {
    let outcome = ActionOutcome::read(
        PageState::new("https://example.com/"),
        json!("Example Domain"),
    )
    .matching("button \"Submit\"");
    let encoded = serde_json::to_value(&outcome).expect("serializes");

    assert_eq!(encoded["value"], json!("Example Domain"));
    assert_eq!(encoded["matched"], json!("button \"Submit\""));
    assert_eq!(
        serde_json::from_value::<ActionOutcome>(encoded).expect("deserializes"),
        outcome
    );
}
