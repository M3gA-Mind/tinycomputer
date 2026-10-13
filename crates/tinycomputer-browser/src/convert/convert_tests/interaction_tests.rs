//! Tests for interactions: refs and selectors, plain actions, typing, scrolling,
//! waiting, and semantic locators.

use serde_json::json;
use tinycomputer_bus::browser::{Action, LocateBy, Locator, ScrollDirection, Target, WaitState};

use super::invalid;
use crate::convert::action;

#[test]
fn refs_and_selectors_become_engine_selectors() {
    let click = action(
        &Action::Click {
            target: Target::parse("@e12"),
            new_tab: true,
        },
        1_000,
    )
    .unwrap();
    assert_eq!(
        click,
        json!({"action": "click", "selector": "@e12", "newTab": true})
    );
    let fill = action(
        &Action::Fill {
            target: Target::selector("#email"),
            value: "a@b.c".to_owned(),
        },
        1_000,
    )
    .unwrap();
    assert_eq!(
        fill,
        json!({"action": "fill", "selector": "#email", "value": "a@b.c"})
    );
}

#[test]
fn every_plain_action_names_its_engine_command() {
    let target = Target::reference("e1");
    let cases = [
        (
            Action::DoubleClick {
                target: target.clone(),
            },
            "dblclick",
        ),
        (
            Action::Hover {
                target: target.clone(),
            },
            "hover",
        ),
        (
            Action::Focus {
                target: target.clone(),
            },
            "focus",
        ),
        (
            Action::Select {
                target: target.clone(),
                values: vec!["SXR".to_owned()],
            },
            "select",
        ),
        (
            Action::Check {
                target: target.clone(),
                checked: true,
            },
            "check",
        ),
        (
            Action::Check {
                target: target.clone(),
                checked: false,
            },
            "uncheck",
        ),
        (
            Action::GetText {
                target: target.clone(),
            },
            "gettext",
        ),
        (
            Action::GetAttribute {
                target: target.clone(),
                attribute: "href".to_owned(),
            },
            "getattribute",
        ),
        (
            Action::IsVisible {
                target: target.clone(),
            },
            "isvisible",
        ),
        (Action::Back, "back"),
        (Action::Forward, "forward"),
        (Action::Reload, "reload"),
        (
            Action::Press {
                key: "Enter".to_owned(),
                target: None,
            },
            "press",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(
            action(&input, 1_000).unwrap()["action"],
            expected,
            "{input:?}"
        );
    }
    assert!(
        invalid(action(
            &Action::Press {
                key: " ".to_owned(),
                target: None,
            },
            1_000
        ))
        .contains("key")
    );
}

#[test]
fn a_press_on_a_target_is_the_plain_key_the_focus_check_precedes() {
    // `Browser::perform` gives the target the focus and checks it first; the
    // key itself goes to the focus, as every press does.
    assert_eq!(
        action(
            &Action::Press {
                key: "Enter".to_owned(),
                target: Some(Target::reference("e4")),
            },
            1_000
        )
        .unwrap(),
        json!({"action": "press", "key": "Enter"})
    );
}

#[test]
fn typing_with_and_without_a_target() {
    let focused = action(
        &Action::Type {
            target: None,
            text: "Srinagar".to_owned(),
            delay_ms: None,
        },
        1_000,
    )
    .unwrap();
    assert_eq!(focused, json!({"action": "inserttext", "text": "Srinagar"}));
    let targeted = action(
        &Action::Type {
            target: Some(Target::reference("e3")),
            text: "SXR".to_owned(),
            delay_ms: Some(40),
        },
        1_000,
    )
    .unwrap();
    assert_eq!(
        targeted,
        json!({"action": "type", "selector": "@e3", "text": "SXR", "delay": 40})
    );
}

#[test]
fn scrolling_maps_directions_and_the_page_ends() {
    let scroll = |direction, pixels, target| {
        action(
            &Action::Scroll {
                direction,
                pixels,
                target,
            },
            1_000,
        )
        .unwrap()
    };
    assert_eq!(
        scroll(ScrollDirection::Down, None, None),
        json!({"action": "scroll", "direction": "down", "amount": 300})
    );
    assert_eq!(scroll(ScrollDirection::Up, Some(50), None)["amount"], 50);
    assert_eq!(
        scroll(ScrollDirection::Left, None, None)["direction"],
        "left"
    );
    assert_eq!(
        scroll(ScrollDirection::Right, None, None)["direction"],
        "right"
    );
    assert_eq!(scroll(ScrollDirection::Top, None, None)["direction"], "up");
    let bottom = scroll(
        ScrollDirection::Bottom,
        None,
        Some(Target::selector(".results")),
    );
    assert_eq!(bottom["direction"], "down");
    assert_eq!(bottom["selector"], ".results");
}

#[test]
fn waiting_prefers_text_then_target_then_a_pause() {
    let wait = |target, text, ms, timeout_ms| {
        action(
            &Action::WaitFor {
                target,
                text,
                state: WaitState::Hidden,
                ms,
                timeout_ms,
            },
            25_000,
        )
    };
    assert_eq!(
        wait(None, Some("Results".to_owned()), None, None).unwrap(),
        json!({"action": "wait", "text": "Results", "timeout": 25_000})
    );
    assert_eq!(
        wait(Some(Target::reference("e2")), None, None, Some(10)).unwrap(),
        json!({"action": "wait", "selector": "@e2", "state": "hidden", "timeout": 10})
    );
    assert_eq!(
        wait(None, None, Some(250), None).unwrap(),
        json!({"action": "wait", "timeout": 250})
    );
    assert!(invalid(wait(None, None, None, None)).contains("wait_for"));
    for state in [WaitState::Attached, WaitState::Detached, WaitState::Visible] {
        let command = action(
            &Action::WaitFor {
                target: Some(Target::reference("e2")),
                text: None,
                state,
                ms: None,
                timeout_ms: None,
            },
            1,
        )
        .unwrap();
        assert_eq!(command["state"], serde_json::to_value(state).unwrap());
    }
}

#[test]
fn locators_use_the_semantic_commands_with_a_subaction() {
    let by = |by, value: &str| Locator::new(by, value);
    let click = action(
        &Action::Click {
            target: Target::locator(by(LocateBy::Role, "button").with_name("Search")),
            new_tab: false,
        },
        1,
    )
    .unwrap();
    assert_eq!(
        click,
        json!({"action": "getbyrole", "role": "button", "exact": false, "subaction": "click", "name": "Search"})
    );
    let fill = action(
        &Action::Fill {
            target: Target::locator(by(LocateBy::Label, "From")),
            value: "Delhi".to_owned(),
        },
        1,
    )
    .unwrap();
    assert_eq!(fill["action"], "getbylabel");
    assert_eq!(fill["value"], "Delhi");
    for (locate, command, key, subaction) in [
        (LocateBy::Text, "getbytext", "text", "hover"),
        (
            LocateBy::Placeholder,
            "getbyplaceholder",
            "placeholder",
            "text",
        ),
        (LocateBy::TestId, "getbytestid", "testId", "check"),
        (LocateBy::AltText, "getbyalttext", "text", "click"),
        (LocateBy::Title, "getbytitle", "text", "click"),
    ] {
        let target = Target::locator(by(locate, "x"));
        let input = match subaction {
            "hover" => Action::Hover { target },
            "text" => Action::GetText { target },
            "check" => Action::Check {
                target,
                checked: true,
            },
            _ => Action::Click {
                target,
                new_tab: false,
            },
        };
        let built = action(&input, 1).unwrap();
        assert_eq!(built["action"], command);
        assert_eq!(built[key], "x");
        assert_eq!(built["subaction"], subaction);
    }
}

#[test]
fn a_locator_the_engine_cannot_combine_is_refused_with_the_alternative() {
    let locator = Target::locator(Locator::new(LocateBy::Text, "Book"));
    let message = invalid(action(
        &Action::Focus {
            target: locator.clone(),
        },
        1,
    ));
    assert!(message.contains("ref or selector"), "{message}");
    let unchecking = invalid(action(
        &Action::Check {
            target: locator,
            checked: false,
        },
        1,
    ));
    assert!(unchecking.contains("uncheck"), "{unchecking}");
    let indexed = Locator {
        index: 2,
        ..Locator::new(LocateBy::Text, "Book")
    };
    assert!(
        invalid(action(
            &Action::Click {
                target: Target::locator(indexed),
                new_tab: false
            },
            1
        ))
        .contains("index")
    );
    assert!(
        invalid(action(
            &Action::Click {
                target: Target::locator(Locator::new(LocateBy::Text, " ")),
                new_tab: false
            },
            1
        ))
        .contains("value")
    );
    assert!(
        invalid(action(
            &Action::Click {
                target: Target::selector(""),
                new_tab: false
            },
            1
        ))
        .contains("non-empty")
    );
}
