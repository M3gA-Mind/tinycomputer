//! Tests pinning the `SiteMemoryHost` interface identity and wire forms.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

#[test]
fn the_interface_is_its_own_not_the_desktop_one() {
    assert_ne!(INTERFACE, crate::names::INTERFACE);
    assert_ne!(OBJECT_PATH, crate::names::OBJECT_PATH);
    assert_eq!(METHODS, ["Load", "Save", "Forget"]);
}

#[test]
fn an_empty_memory_round_trips_and_reads_as_empty() {
    let memory: SiteMemory = serde_json::from_str("{}").unwrap();
    assert!(memory.is_empty());
    let reply: LoadSiteResponse = serde_json::from_str("{}").unwrap();
    assert_eq!(reply, LoadSiteResponse::default());
}

#[test]
fn a_saved_plan_and_hint_keep_their_wire_form() {
    let memory: SiteMemory = serde_json::from_value(serde_json::json!({
        "plans": [{"goal": "order milk", "facts_id": "ab", "flow": {"app": "browser", "steps": ["a"]},
                   "ok_at": 5, "runs": 2}],
        "hints": [{"hint": {"app": "browser", "key": "k", "role": "button", "name": "Go"}, "ok_at": 7}]
    }))
    .unwrap();
    assert_eq!(memory.plans[0].runs, 2);
    assert_eq!(memory.hints[0].ok_at, 7);
    let again: SiteMemory = serde_json::from_value(serde_json::to_value(&memory).unwrap()).unwrap();
    assert_eq!(again, memory);
}

#[test]
fn forgetting_without_a_site_means_every_site() {
    let request: ForgetSiteRequest = serde_json::from_str("{}").unwrap();
    assert_eq!(request.site, None);
}
