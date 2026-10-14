//! The recorded `browser` tool schema and its argument parsing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::json;

fn args(value: serde_json::Value) -> ToolArgs {
    serde_json::from_value(value).unwrap()
}

#[test]
fn the_recorded_schema_matches_the_literal_the_host_shipped() {
    let expected = json!({"type":"object","properties":{
        "action":{"type":"string","enum":["open","snapshot","read_page","click","fill","type","get_text","get_title","get_url","wait","press","hover","scroll","is_visible","find","task","task_continue","task_cancel","confirm_pending","list_downloads","wait_download","close"]},
        "url":{"type":"string","description":"Starting HTTPS URL for open or an optional starting URL for task"},"selector":{"type":"string"},"value":{"type":"string"},"text":{"type":"string"},"key":{"type":"string"},"direction":{"type":"string"},"pixels":{"type":"integer"},"ms":{"type":"integer"},"timeout_ms":{"type":"integer"},"interactive_only":{"type":"boolean"},"compact":{"type":"boolean"},"depth":{"type":"integer"},"by":{"type":"string"},"find_action":{"type":"string"},"fill_value":{"type":"string"},"goal":{"type":"string"},"inputs":{"type":"object","additionalProperties":{"type":"string"}},"task_id":{"type":"string","description":"Task id returned by task, for task_continue and task_cancel"},"flow":{"type":"object","description":"Optional TinyComputer flow ({app, vars, steps}) to run instead of planning one from goal, e.g. a plan saved from an earlier successful run"},"answer":{"type":"string","description":"Free-text answer for a paused task; done after a needs_human pause"},"token":{"type":"string","description":"Token returned with the exact pending action"}
    },"required":["action"]});
    assert_eq!(parameters_schema(), expected);
    assert_eq!(TOOL_NAME, "browser");
}

#[test]
fn every_schema_action_is_a_tool_action_and_back() {
    let schema = parameters_schema();
    let names: Vec<&str> = schema["properties"]["action"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 22);
    for name in names {
        let action: ToolAction = serde_json::from_value(json!(name)).unwrap();
        assert_eq!(action.as_str(), name);
    }
}

#[test]
fn every_arg_field_is_a_schema_property() {
    let full = serde_json::to_value(ToolArgs {
        action: Some(ToolAction::Open),
        url: Some(String::new()),
        selector: Some(String::new()),
        value: Some(String::new()),
        text: Some(String::new()),
        key: Some(String::new()),
        direction: Some(String::new()),
        pixels: Some(0),
        ms: Some(0),
        timeout_ms: Some(0),
        interactive_only: Some(true),
        compact: Some(true),
        depth: Some(0),
        by: Some(String::new()),
        find_action: Some(String::new()),
        fill_value: Some(String::new()),
        goal: Some(String::new()),
        inputs: Some(BTreeMap::default()),
        task_id: Some(String::new()),
        flow: Some(json!({})),
        answer: Some(String::new()),
        token: Some(String::new()),
    })
    .unwrap();
    let schema = parameters_schema();
    let props = schema["properties"].as_object().unwrap();
    let fields = full.as_object().unwrap();
    assert_eq!(fields.len(), props.len());
    for key in fields.keys() {
        assert!(props.contains_key(key), "{key} is not in the schema");
    }
}

#[test]
fn click_fill_and_type_parse_their_targets() {
    assert_eq!(
        args(json!({"action":"click","selector":"@e3"}))
            .to_action()
            .unwrap(),
        Action::Click {
            target: Target::parse("@e3"),
            new_tab: false
        }
    );
    assert_eq!(
        args(json!({"action":"fill","selector":"#q","value":"hi"}))
            .to_action()
            .unwrap(),
        Action::Fill {
            target: Target::parse("#q"),
            value: "hi".into()
        }
    );
    assert_eq!(
        args(json!({"action":"type","text":"hi"}))
            .to_action()
            .unwrap(),
        Action::Type {
            target: None,
            text: "hi".into(),
            delay_ms: None
        }
    );
}

#[test]
fn a_press_takes_the_selector_as_its_target_when_one_is_given() {
    assert_eq!(
        args(json!({"action":"press","key":"Enter"}))
            .to_action()
            .unwrap(),
        Action::Press {
            key: "Enter".into(),
            target: None
        }
    );
    assert_eq!(
        args(json!({"action":"press","key":"Enter","selector":"@e5"}))
            .to_action()
            .unwrap(),
        Action::Press {
            key: "Enter".into(),
            target: Some(Target::parse("@e5"))
        }
    );
}

#[test]
fn a_blank_required_parameter_is_missing() {
    let err = args(json!({"action":"click","selector":"  "}))
        .to_action()
        .unwrap_err();
    assert_eq!(err, ToolArgError::Missing("selector"));
    assert_eq!(err.to_string(), "Missing 'selector' parameter");
    assert_eq!(
        ToolArgs::default().to_action().unwrap_err(),
        ToolArgError::Missing("action")
    );
}

#[test]
fn scroll_accepts_four_directions_and_drops_an_oversized_pixel_count() {
    let scroll = args(json!({"action":"scroll","direction":"left","pixels":u64::MAX}));
    assert_eq!(
        scroll.to_action().unwrap(),
        Action::Scroll {
            direction: ScrollDirection::Left,
            pixels: None,
            target: None
        }
    );
    let err = args(json!({"action":"scroll","direction":"top"}))
        .to_action()
        .unwrap_err();
    assert_eq!(err.to_string(), "Invalid direction: top");
}

#[test]
fn wait_carries_optional_target_text_and_delays() {
    assert_eq!(
        args(json!({"action":"wait","ms":10,"timeout_ms":99}))
            .to_action()
            .unwrap(),
        Action::WaitFor {
            target: None,
            text: None,
            state: WaitState::Visible,
            ms: Some(10),
            timeout_ms: Some(99)
        }
    );
}

#[test]
fn find_builds_a_locator_for_each_follow_up() {
    let target = Target::locator(Locator::new(LocateBy::TestId, "go"));
    let find = |follow: &str| {
        args(json!({"action":"find","by":"testid","value":"go","find_action":follow,"fill_value":"x"}))
            .to_action()
    };
    assert_eq!(
        find("click").unwrap(),
        Action::Click {
            target: target.clone(),
            new_tab: false
        }
    );
    assert_eq!(
        find("fill").unwrap(),
        Action::Fill {
            target: target.clone(),
            value: "x".into()
        }
    );
    assert_eq!(
        find("text").unwrap(),
        Action::GetText {
            target: target.clone()
        }
    );
    assert_eq!(find("hover").unwrap(), Action::Hover { target });
    assert_eq!(
        find("drag").unwrap_err().to_string(),
        "Invalid find action: drag"
    );
    let bad = args(json!({"action":"find","by":"css","value":"a","find_action":"click"}));
    assert_eq!(
        bad.to_action().unwrap_err().to_string(),
        "Invalid locator: css"
    );
}

#[test]
fn session_and_task_actions_are_not_page_actions() {
    for name in ["open", "task", "close", "snapshot", "confirm_pending"] {
        let err = args(json!({"action": name})).to_action().unwrap_err();
        assert!(matches!(err, ToolArgError::Unsupported(n) if n == name));
    }
}

#[test]
fn task_inputs_must_be_text() {
    let ok = args(json!({"action":"task","goal":"g","inputs":{"a":"b"}}));
    assert_eq!(ok.inputs.unwrap()["a"], "b");
    assert!(serde_json::from_value::<ToolArgs>(json!({"action":"task","inputs":{"a":1}})).is_err());
}
