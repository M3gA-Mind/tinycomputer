//! Tests for deep controls: reading a `d` ref, what one takes, the refs a
//! session hands out, and placing the controls in a snapshot's tree.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod live_tests;
mod merge_tests;

use serde_json::{Value, json};
use tinycomputer_bus::browser::{Action, Target};

use tinycomputer_bus::browser::{ScrollDirection, WaitState};

use super::{Control, Kind, Minted, aimed_at, number, supports, unsupported};
use crate::error::Error;

#[test]
fn a_deep_ref_is_d_and_digits_with_or_without_its_at() {
    assert_eq!(number(&Target::reference("d3")), Some(3));
    assert_eq!(number(&Target::reference("@d12")), Some(12));
    assert_eq!(number(&Target::parse("@d7")), Some(7));
    for other in ["e3", "d", "dx", "d3a", "seen:4", ""] {
        assert_eq!(number(&Target::reference(other)), None, "{other}");
    }
    assert_eq!(
        number(&Target::selector("d3")),
        None,
        "a selector is no ref"
    );
}

#[test]
fn an_action_is_aimed_at_the_deep_ref_it_targets() {
    let deep = || Target::reference("d2");
    assert_eq!(
        aimed_at(&Action::Click {
            target: deep(),
            new_tab: false
        }),
        Some(2)
    );
    assert_eq!(
        aimed_at(&Action::Press {
            key: "Enter".to_owned(),
            target: Some(deep()),
        }),
        Some(2)
    );
    assert_eq!(
        aimed_at(&Action::Type {
            target: None,
            text: "x".to_owned(),
            delay_ms: None,
        }),
        None
    );
    assert_eq!(
        aimed_at(&Action::Click {
            target: Target::reference("e2"),
            new_tab: false
        }),
        None
    );
    assert_eq!(aimed_at(&Action::Back), None);
}

#[test]
fn every_action_is_aimed_by_its_target_and_named_when_refused() {
    let deep = || Target::reference("d5");
    let aimed = [
        (
            Action::Click {
                target: deep(),
                new_tab: false,
            },
            "click",
        ),
        (Action::DoubleClick { target: deep() }, "double_click"),
        (Action::Hover { target: deep() }, "hover"),
        (Action::Focus { target: deep() }, "focus"),
        (
            Action::Fill {
                target: deep(),
                value: String::new(),
            },
            "fill",
        ),
        (
            Action::Type {
                target: Some(deep()),
                text: String::new(),
                delay_ms: None,
            },
            "type",
        ),
        (
            Action::Press {
                key: "Enter".to_owned(),
                target: Some(deep()),
            },
            "press",
        ),
        (
            Action::Select {
                target: deep(),
                values: Vec::new(),
            },
            "select",
        ),
        (
            Action::Check {
                target: deep(),
                checked: true,
            },
            "check",
        ),
        (
            Action::Scroll {
                direction: ScrollDirection::Down,
                pixels: None,
                target: Some(deep()),
            },
            "scroll",
        ),
        (Action::GetText { target: deep() }, "get_text"),
        (
            Action::GetAttribute {
                target: deep(),
                attribute: "href".to_owned(),
            },
            "get_attribute",
        ),
        (Action::IsVisible { target: deep() }, "is_visible"),
        (
            Action::WaitFor {
                target: Some(deep()),
                text: None,
                state: WaitState::Visible,
                ms: None,
                timeout_ms: None,
            },
            "wait_for",
        ),
    ];
    for (action, verb) in &aimed {
        assert_eq!(aimed_at(action), Some(5), "{verb}");
        assert!(
            unsupported(action)
                .to_string()
                .contains(&format!("{verb} does not take")),
            "{verb}"
        );
    }
    for (action, verb) in [
        (Action::Back, "back"),
        (Action::Forward, "forward"),
        (Action::Reload, "reload"),
    ] {
        assert_eq!(aimed_at(&action), None);
        assert!(
            unsupported(&action)
                .to_string()
                .contains(&format!("{verb} does not take"))
        );
    }
}

#[test]
fn a_deep_ref_takes_presses_text_and_reads_only() {
    let deep = || Target::reference("d1");
    for action in [
        Action::Click {
            target: deep(),
            new_tab: false,
        },
        Action::Hover { target: deep() },
        Action::Focus { target: deep() },
        Action::Fill {
            target: deep(),
            value: "x".to_owned(),
        },
        Action::GetText { target: deep() },
        Action::IsVisible { target: deep() },
    ] {
        assert!(supports(&action).is_ok(), "{action:?}");
    }
    let refused = supports(&Action::Select {
        target: deep(),
        values: vec!["a".to_owned()],
    })
    .unwrap_err();
    assert!(
        matches!(&refused, Error::InvalidInput { message } if message.starts_with("select does not take a deep ref")),
        "{refused}"
    );
    assert!(matches!(
        supports(&Action::Click {
            target: deep(),
            new_tab: true
        }),
        Err(Error::InvalidInput { .. })
    ));
    assert!(matches!(
        supports(&Action::Press {
            key: " ".to_owned(),
            target: Some(deep()),
        }),
        Err(Error::InvalidInput { .. })
    ));
}

#[test]
fn a_control_reads_as_a_line_of_the_tree() {
    let named = Control {
        number: 4,
        kind: Kind::Clickable,
        text: "Say \"hi\"".to_owned(),
    };
    assert_eq!(named.line(2), "    - clickable \"Say \\\"hi\\\"\" [ref=d4]");
    let nameless = Control {
        number: 9,
        kind: Kind::Editable,
        text: String::new(),
    };
    assert_eq!(nameless.line(0), "- editable [ref=d9]");
    let tab_stop = Control {
        number: 10,
        kind: Kind::Focusable,
        text: "Ride options".to_owned(),
    };
    assert_eq!(tab_stop.line(1), "  - focusable \"Ride options\" [ref=d10]");
}

/// A look's reply listing `found`, minting from `next`.
fn look(found: &Value, next: u64) -> Value {
    json!({"found": found, "next": next})
}

/// The options a look's script is called with.
fn options(script: &str) -> Value {
    let call = script
        .rfind(")(\"find\", ")
        .expect("a look calls the script with find");
    let options = &script[call + ")(\"find\", ".len()..script.len() - 1];
    serde_json::from_str(options).expect("the options are JSON")
}

#[test]
fn a_session_hands_out_what_its_latest_look_found() {
    let mut minted = Minted::default();
    assert!(!minted.holds(1));
    assert_eq!(
        options(&minted.look(None)),
        json!({"keep": [], "longest": 100, "most": 40, "next": 1, "scope": null})
    );
    let controls = minted.take(Some(&look(
        &json!([
            {"n": 1, "kind": "clickable", "text": "  LOG\n IN "},
            {"n": 2, "kind": "editable", "text": ""},
        ]),
        3,
    )));
    assert_eq!(
        controls,
        vec![
            Control {
                number: 1,
                kind: Kind::Clickable,
                text: "LOG IN".to_owned()
            },
            Control {
                number: 2,
                kind: Kind::Editable,
                text: String::new()
            },
        ]
    );
    assert!(minted.holds(1) && minted.holds(2));
    assert_eq!(
        options(&minted.look(Some("#app"))),
        json!({"keep": [1, 2], "longest": 100, "most": 40, "next": 3, "scope": "#app"})
    );
    // A look that does not reply hands out none: the refs before are stale.
    assert!(minted.take(None).is_empty());
    assert!(!minted.holds(1));
    assert!(minted.take(Some(&json!(42))).is_empty());
    // Numbers never go back, even when the page reports a lower next.
    minted.take(Some(&look(&json!([]), 1)));
    assert_eq!(options(&minted.look(None))["next"], 3);
}

#[test]
fn a_look_reply_is_read_defensively() {
    let mut minted = Minted::default();
    let long = "w".repeat(150);
    let controls = minted.take(Some(&look(
        &json!([
            {"n": 0, "kind": "clickable", "text": "zero"},
            {"n": 5, "kind": "button", "text": "unknown kind"},
            {"n": 6, "kind": "focusable", "text": long},
            {"n": 6, "kind": "clickable", "text": "twice"},
            {"kind": "clickable", "text": "no number"},
        ]),
        2,
    )));
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].number, 6);
    assert_eq!(controls[0].kind, Kind::Focusable);
    assert_eq!(controls[0].text.chars().count(), 100);
    // Past every number found, whatever the page said.
    assert_eq!(options(&minted.look(None))["next"], 7);
    let many = (1..=50)
        .map(|n| json!({"n": n, "kind": "clickable", "text": format!("row {n}")}))
        .collect::<Vec<_>>();
    assert_eq!(minted.take(Some(&look(&json!(many), 51))).len(), 40);
}
