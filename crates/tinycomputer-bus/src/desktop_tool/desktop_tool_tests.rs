//! The desktop tool declarations and request builders.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::json;

fn expected_schemas() -> Vec<(DesktopTool, serde_json::Value)> {
    vec![
        (DesktopTool::Apps, json!({"type":"object","properties":{}})),
        (
            DesktopTool::Windows,
            json!({"type":"object","properties":{
                "app":{"type":"string","description":"Optional application name"}}}),
        ),
        (
            DesktopTool::Launch,
            json!({"type":"object","properties":{
                "app":{"type":"string","description":"Application name, e.g. Spotify or TextEdit"}},
                "required":["app"],"additionalProperties":false}),
        ),
        (
            DesktopTool::Snapshot,
            json!({"type":"object","properties":{
                "app":{"type":"string"}, "window_id":{"type":"string","minLength":1,"description":"Exact window ID from desktop_list_windows"}, "skeleton":{"type":"boolean"},
                "root_ref":{"type":"string"}, "max_depth":{"type":"integer","minimum":1,"maximum":12}}}),
        ),
        (
            DesktopTool::Find,
            json!({"type":"object","properties":{
                "app":{"type":"string"},"role":{"type":"string"},"name":{"type":"string"},
                "root":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":20}}}),
        ),
        (
            DesktopTool::Act,
            json!({"type":"object","properties":{
                "operation":{"type":"string","enum":["click","focus","type","check","uncheck","expand","collapse"]},
                "ref_id":{"type":"string","description":"Snapshot-qualified ref from desktop_snapshot or desktop_find"},
                "text":{"type":"string","description":"Required for type"}},
                "required":["operation","ref_id"]}),
        ),
        (
            DesktopTool::Goal,
            json!({"type":"object","properties":{
                "app":{"type":"string","description":"Native app to control"},
                "window":{"type":"string","description":"Optional exact title of an observed app window"},
                "window_id":{"type":"string","minLength":1,"description":"Optional exact window ID from desktop_list_windows; binds actions and verification to that native window"},
                "goal":{"type":"string","description":"One bounded desktop task; describe the intended visible result"},
                "allowed_operations":{"type":"array","minItems":1,"items":{"type":"string","enum":["CLICK","TYPE_TEXT","CHECK","UNCHECK","EXPAND","COLLAPSE","SCROLL"]},"description":"Mutating operations Jev may execute; enumerate those needed for this task"},
                "allowed_targets":{"type":"array","minItems":1,"items":{"type":"string"},"description":"Exact accessible name, description, or native_id.value from desktop_snapshot or desktop_find for each action target"},
                "text_slots":{"type":"object","additionalProperties":{"type":"string"},"description":"Prepared text keyed by an exact allowed target. Copy user supplied text verbatim; do not normalize punctuation, spacing, or case"},
                "success":{"type":"array","minItems":1,"items":{"type":"object","properties":{
                    "kind":{"type":"string","enum":["name_present","name_contains","value_equals","value_contains","state_contains"]},
                    "name":{"type":"string","description":"Exact accessible name, description, or native_id.value from the snapshot for name_present, value_equals, value_contains, or state_contains"},
                    "fragment":{"type":"string","description":"For name_contains, a stable substring of the visible descendant name, such as the exact outgoing message text"},
                    "within":{"type":"string","description":"For name_contains, the exact accessible name of the ancestor container, such as the active conversation message list"},
                    "value":{"type":"string"},"state":{"type":"string"}},
                    "required":["kind"]},"description":"All predicates must match a fresh accessibility observation before completion. name_contains requires fragment and within"},
                "max_steps":{"type":"integer","minimum":1,"maximum":20},
                "max_model_calls":{"type":"integer","minimum":1,"maximum":40},
                "max_elapsed_ms":{"type":"integer","minimum":1000,"maximum":300_000}},
                "required":["app","goal","allowed_operations","allowed_targets","success"],"additionalProperties":false}),
        ),
        (
            DesktopTool::ContinueGoal,
            json!({"type":"object","properties":{
                "confirmation_id":{"type":"string","description":"One-use handle approved by the user in Connections"}},
                "required":["confirmation_id"]}),
        ),
    ]
}

#[test]
fn desktop_static_schemas_match_json_literals() {
    for (tool, expected) in expected_schemas() {
        assert_eq!(tool.parameters_schema(), expected, "{}", tool.name());
    }
}

#[test]
fn every_tool_is_listed_once_and_round_trips_its_name() {
    assert_eq!(expected_schemas().len(), DesktopTool::ALL.len());
    for tool in DesktopTool::ALL {
        assert_eq!(DesktopTool::from_name(tool.name()), Some(tool));
        assert!(!tool.description().is_empty());
        assert_ne!(tool.parameters_schema(), Value::Null);
    }
    assert_eq!(DesktopTool::from_name("desktop_nope"), None);
}

#[test]
fn desktop_arguments_reject_missing_or_blank_required_values() {
    assert_eq!(
        required(&json!({"app":"  TextEdit  "}), "app").unwrap(),
        "TextEdit"
    );
    for args in [json!({}), json!({"app":"  "}), json!({"app":42})] {
        assert!(required(&args, "app")
            .unwrap_err()
            .to_string()
            .contains("missing required parameter: app"));
    }
}

#[test]
fn launch_tool_cannot_accept_process_arguments_or_environment() {
    let schema = DesktopTool::Launch.parameters_schema();
    assert!(DesktopTool::Launch.mutates());
    assert_eq!(schema["required"], json!(["app"]));
    assert!(schema["properties"].get("args").is_none());
    assert!(schema["properties"].get("env").is_none());
    assert!(schema["properties"].get("cdp_port").is_none());
    let (member, request) = launch_request(&json!({"app":" TextEdit "})).unwrap();
    assert_eq!(member, names::methods::LAUNCH);
    assert_eq!(request.attach_if_running, Some(true));
    assert!(request.activate);
    assert!(launch_request(&json!({})).is_err());
}

#[test]
fn raw_ref_action_requires_a_snapshot_ref_and_declares_a_write_effect() {
    let schema = DesktopTool::Act.parameters_schema();
    assert_eq!(DesktopTool::Act.name(), "desktop_act");
    assert!(DesktopTool::Act.mutates());
    assert_eq!(schema["required"], json!(["operation", "ref_id"]));
    assert_eq!(
        schema["properties"]["operation"]["enum"],
        json!(["click", "focus", "type", "check", "uncheck", "expand", "collapse"])
    );
    assert!(schema["properties"].get("args").is_none());
    assert!(schema["properties"].get("env").is_none());
}

#[test]
fn act_maps_each_operation_to_its_member() {
    for (operation, member) in [
        ("click", names::methods::CLICK),
        ("focus", names::methods::FOCUS),
        ("check", names::methods::CHECK),
        ("uncheck", names::methods::UNCHECK),
        ("expand", names::methods::EXPAND),
        ("collapse", names::methods::COLLAPSE),
    ] {
        let (got, request) = act_request(&json!({"ref_id":"r1","operation":operation})).unwrap();
        assert_eq!(got, member);
        assert!(matches!(request, ActRequest::Ref(_)));
    }
    let (member, request) =
        act_request(&json!({"ref_id":"r1","operation":"type","text":"hi"})).unwrap();
    assert_eq!(member, names::methods::TYPE);
    assert!(matches!(request, ActRequest::Type(t) if t.text == "hi" && t.ref_id == "r1"));
    assert_eq!(
        act_request(&json!({"ref_id":"r1","operation":"type"})).unwrap_err(),
        ToolArgError::Missing("text")
    );
    assert_eq!(
        act_request(&json!({"ref_id":"r1","operation":"drag"})).unwrap_err(),
        ToolArgError::UnsupportedOperation
    );
    assert_eq!(
        act_request(&json!({"operation":"click"})).unwrap_err(),
        ToolArgError::Missing("ref_id")
    );
}

#[test]
fn snapshot_can_bind_the_window_listed_by_desktop() {
    assert!(DesktopTool::Snapshot.parameters_schema()["properties"]
        .get("window_id")
        .is_some());
    let (member, request) =
        snapshot_request(&json!({"app":"TextEdit","window_id":"w-515619"})).unwrap();
    assert_eq!(member, names::methods::SNAPSHOT);
    assert_eq!(request.app.as_deref(), Some("TextEdit"));
    assert_eq!(request.window_id.as_deref(), Some("w-515619"));
    assert!(request.skeleton);
    assert!(snapshot_request(&json!({"window_id":"  "})).is_err());
    let (_, deep) = snapshot_request(&json!({"max_depth":99})).unwrap();
    assert_eq!(deep.max_depth, Some(12));
}

#[test]
fn find_limit_is_defaulted_and_clamped() {
    assert_eq!(find_request(&json!({})).1.limit, Some(10));
    assert_eq!(find_request(&json!({"limit":500})).1.limit, Some(20));
    assert_eq!(find_request(&json!({"limit":0})).1.limit, Some(1));
    let (member, request) = find_request(&json!({"role":"button","name":"Play"}));
    assert_eq!(member, names::methods::FIND);
    assert_eq!(request.role.as_deref(), Some("button"));
}

#[test]
fn listing_requests_use_their_members() {
    assert_eq!(apps_request().0, names::methods::LIST_APPS);
    let (member, request) = windows_request(&json!({"app":"Mail"}));
    assert_eq!(member, names::methods::LIST_WINDOWS);
    assert_eq!(request.app.as_deref(), Some("Mail"));
    assert!(!DesktopTool::Apps.needs_accessibility());
    assert!(DesktopTool::Find.needs_accessibility());
}

#[test]
fn goal_and_continuation_have_distinct_required_inputs() {
    let goal = DesktopTool::Goal.parameters_schema();
    let continuation = DesktopTool::ContinueGoal.parameters_schema();
    assert_eq!(
        goal["required"],
        json!([
            "app",
            "goal",
            "allowed_operations",
            "allowed_targets",
            "success"
        ])
    );
    assert!(goal["properties"].get("confirmation_id").is_none());
    let kinds = goal["properties"]["success"]["items"]["properties"]["kind"]["enum"]
        .as_array()
        .unwrap();
    assert!(!kinds.contains(&json!("name_absent")));
    assert!(kinds.contains(&json!("name_contains")));
    assert_eq!(continuation["required"], json!(["confirmation_id"]));
    assert!(continuation["properties"].get("approve").is_none());
}

#[test]
fn goal_request_carries_scoped_task_and_disables_confirmations() {
    let args = json!({
        "app":"TextEdit", "window":"Untitled", "window_id":"w-515619",
        "goal":"Enter a disposable marker",
        "allowed_operations":["TYPE_TEXT"], "allowed_targets":["Text Entry Area"],
        "text_slots":{"Text Entry Area":"desktop-test-42"},
        "success":[{"kind":"value_equals","name":"Text Entry Area","value":"desktop-test-42"}],
        "max_steps":3, "max_model_calls":5, "max_elapsed_ms":20_000
    });
    let (member, request) = goal_request(&args, false).unwrap();
    assert_eq!(member, names::methods::RUN_GOAL);
    let wire = serde_json::to_value(&request).unwrap();
    assert!(!request.require_confirmations);
    assert_eq!(request.window.as_deref(), Some("Untitled"));
    assert_eq!(request.window_id.as_deref(), Some("w-515619"));
    assert_eq!(wire["allowed_operations"], json!(["TYPE_TEXT"]));
    assert_eq!(wire["allowed_targets"], json!(["Text Entry Area"]));
    assert_eq!(wire["text_slots"]["Text Entry Area"], "desktop-test-42");
    assert_eq!(wire["success"], args["success"]);
    assert_eq!(request.max_elapsed_ms, 20_000);
    assert!(request.continuation.is_none());
    assert!(!request.include_values);
    assert!(goal_request(&args, true).unwrap().1.require_confirmations);
    assert!(goal_request(&json!({"app":"TextEdit","goal":"type"}), false).is_err());

    let without_targets = json!({"app":"TextEdit","goal":"type", "allowed_operations":["TYPE_TEXT"],
        "success":[{"kind":"name_present","name":"Text Entry Area"}]});
    assert!(goal_request(&without_targets, false)
        .unwrap_err()
        .to_string()
        .contains("needs_inspection"));
    let mut blank_target = without_targets.clone();
    blank_target["allowed_targets"] = json!(["  "]);
    assert!(goal_request(&blank_target, false).is_err());
    let mut blank_window = args.clone();
    blank_window["window_id"] = json!("  ");
    assert!(goal_request(&blank_window, false).is_err());
    let malformed = json!({"app":"TextEdit","goal":"type", "allowed_operations":["TYPE_TEXT"],
        "allowed_targets":["Text Entry Area"],
        "success":[{"kind":"value_contains","name":"Text Entry Area"}]});
    assert!(goal_request(&malformed, false).is_err());
    for predicate in [
        json!({"kind":"name_absent","name":"Play"}),
        json!({"kind":"name_present","name":"  "}),
        json!({"kind":"value_contains","name":"Text Entry Area","value":""}),
        json!({"kind":"value_equals","name":"","value":"marker"}),
        json!({"kind":"state_contains","name":"Play","state":" "}),
    ] {
        let mut request = args.clone();
        request["success"] = json!([predicate]);
        assert!(goal_request(&request, false).is_err());
    }
    let mut cleared = args.clone();
    cleared["success"] = json!([{"kind":"value_equals","name":"Text Entry Area","value":""}]);
    assert!(goal_request(&cleared, false).is_ok());

    let outbound = json!({
        "app":"WhatsApp", "goal":"Send exactly the prepared text to Alex Rivera",
        "allowed_operations":["CLICK","TYPE_TEXT"],
        "allowed_targets":["Compose message","Send"],
        "text_slots":{"Compose message":"Hello from OpenHuman!  "},
        "success":[
            {"kind":"name_present","name":"Messages in chat with Alex Rivera"},
            {"kind":"name_contains","fragment":"Your message, Hello from OpenHuman!  ","within":"Messages in chat with Alex Rivera"}
        ]
    });
    let prepared = serde_json::to_value(goal_request(&outbound, false).unwrap().1).unwrap();
    assert_eq!(
        prepared["text_slots"]["Compose message"],
        "Hello from OpenHuman!  "
    );
    assert_eq!(
        prepared["success"][1]["fragment"],
        "Your message, Hello from OpenHuman!  "
    );
    let mut wrong_slot = outbound.clone();
    wrong_slot["text_slots"] = json!({"Other field":"Hello from OpenHuman!  "});
    assert!(goal_request(&wrong_slot, false).is_err());
    let mut missing_scope = outbound;
    missing_scope["success"][1]["within"] = json!(" ");
    assert!(goal_request(&missing_scope, false).is_err());
}

#[test]
fn goal_timeout_outlives_the_module_loop_budget() {
    assert_eq!(DesktopTool::Goal.timeout_ms(&json!({})), Some(150_000));
    assert_eq!(
        DesktopTool::Goal.timeout_ms(&json!({"max_elapsed_ms":300_000})),
        Some(330_000)
    );
    assert_eq!(
        DesktopTool::Goal.timeout_ms(&json!({"max_elapsed_ms":999_999})),
        Some(330_000)
    );
    assert_eq!(
        DesktopTool::ContinueGoal.timeout_ms(&json!({})),
        Some(330_000)
    );
    assert_eq!(DesktopTool::Find.timeout_ms(&json!({})), None);
}
