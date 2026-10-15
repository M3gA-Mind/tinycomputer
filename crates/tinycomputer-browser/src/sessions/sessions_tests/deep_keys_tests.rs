//! Tests for keys and reads on deep controls: text typed and filled where a
//! press put the focus, keys refused anywhere else, and the marked element
//! focused and read, over a scripted engine.

use serde_json::json;
use tinycomputer_bus::browser::{Action, SnapshotRequest, Target};

use super::deep_page::{mouse, ops, page, shown};
use super::open;
use crate::error::Error;

#[tokio::test]
async fn text_is_typed_where_the_press_put_the_focus() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-type").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    browser
        .perform(
            &id,
            Action::Type {
                target: Some(Target::reference("d2")),
                text: "Kora".to_owned(),
                delay_ms: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(ops(&fake), ["find", "aim", "aim", "end"]);
    assert_eq!(mouse(&fake).len(), 3, "pressed into the field");
    assert_eq!(
        fake.last("keyboard"),
        json!({"action": "keyboard", "subaction": "type", "text": "Kora"})
    );
    // The focus came off whatever held it before the press.
    let actions = fake.actions();
    let blurred = fake
        .sent()
        .iter()
        .position(|command| {
            command["action"] == "evaluate"
                && command["script"]
                    .as_str()
                    .is_some_and(|script| script.contains("element.blur()"))
        })
        .expect("the focus was taken off");
    let pressed = actions.iter().position(|action| action == "mouse").unwrap();
    assert!(blurred < pressed);
}

#[tokio::test]
async fn typing_with_a_delay_sends_a_character_at_a_time() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-type-slow").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    browser
        .perform(
            &id,
            Action::Type {
                target: Some(Target::reference("d2")),
                text: "ab".to_owned(),
                delay_ms: Some(1),
            },
        )
        .await
        .unwrap();
    let typed: Vec<_> = fake
        .sent()
        .iter()
        .filter(|command| command["action"] == "keyboard")
        .map(|command| command["text"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(typed, ["a", "b"]);
}

#[tokio::test]
async fn a_fill_replaces_what_the_field_held() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-fill").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let fill = |value: &str| Action::Fill {
        target: Target::reference("d2"),
        value: value.to_owned(),
    };
    browser.perform(&id, fill("Airport")).await.unwrap();
    assert_eq!(ops(&fake), ["find", "aim", "aim", "select"]);
    assert_eq!(
        fake.last("inserttext"),
        json!({"action": "inserttext", "text": "Airport"})
    );
    browser.perform(&id, fill("")).await.unwrap();
    assert_eq!(
        fake.last("press"),
        json!({"action": "press", "key": "Backspace"})
    );
}

#[tokio::test]
async fn keys_never_go_anywhere_the_action_does_not_point() {
    // The press left the focus on a field outside the element.
    let elsewhere = page(|op| match op {
        "aim" => json!({"x": 40.0, "y": 12.0, "field": true}),
        "end" | "select" | "focus" => json!({"inside": false, "on": "input"}),
        _ => json!({"gone": true}),
    });
    let (browser, id) = open(&elsewhere, "deep-focus-missed").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let typed = Action::Type {
        target: Some(Target::reference("d1")),
        text: "secret".to_owned(),
        delay_ms: None,
    };
    assert!(matches!(
        browser.perform(&id, typed).await,
        Err(Error::FocusMissed { focused }) if focused == "input"
    ));
    let pressed = Action::Press {
        key: "Enter".to_owned(),
        target: Some(Target::reference("d1")),
    };
    assert!(matches!(
        browser.perform(&id, pressed).await,
        Err(Error::FocusMissed { .. })
    ));
    assert!(
        !elsewhere
            .actions()
            .iter()
            .any(|action| action == "keyboard" || action == "press"),
        "no key was sent"
    );
    // Nothing in the element takes text: it is not even pressed.
    let no_field = page(|op| match op {
        "aim" => json!({"x": 40.0, "y": 12.0, "field": false}),
        _ => json!({"gone": true}),
    });
    let (browser, id) = open(&no_field, "deep-no-field").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let filled = || Action::Fill {
        target: Target::reference("d1"),
        value: "x".to_owned(),
    };
    assert!(matches!(
        browser.perform(&id, filled()).await,
        Err(Error::NotActionable { reason }) if reason == "@d1 takes no typed text: there is no field or editable region in it, so no text was sent"
    ));
    assert!(mouse(&no_field).is_empty(), "nothing was pressed");
    let blurred = no_field.sent().iter().any(|command| {
        command["script"]
            .as_str()
            .is_some_and(|script| script.contains("element.blur()"))
    });
    assert!(!blurred, "the focus stayed where it was");
    // It holds one, but the press focused something in it that takes none.
    let no_text = page(|op| match op {
        "aim" => json!({"x": 40.0, "y": 12.0, "field": true}),
        "select" => json!({"inside": true, "on": "div", "takes": false}),
        _ => json!({"gone": true}),
    });
    let (browser, id) = open(&no_text, "deep-no-text").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert!(matches!(
        browser.perform(&id, filled()).await,
        Err(Error::NotActionable { reason }) if reason.contains("pressing it focused no field")
    ));
    assert!(
        !no_text
            .actions()
            .iter()
            .any(|action| action == "inserttext")
    );
    // Gone once pressed: no stale ref, since the press reached the page.
    let vanished = page(|op| match op {
        "aim" => json!({"x": 40.0, "y": 12.0, "field": true}),
        _ => json!({"gone": true}),
    });
    let (browser, id) = open(&vanished, "deep-vanished").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let typed = Action::Type {
        target: Some(Target::reference("d1")),
        text: "x".to_owned(),
        delay_ms: None,
    };
    assert!(matches!(
        browser.perform(&id, typed).await,
        Err(Error::NotActionable { reason }) if reason.contains("went away")
    ));
}

#[tokio::test]
async fn focus_press_and_reads_reach_the_marked_element() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-reads").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let deep = || Target::reference("d1");
    browser
        .perform(&id, Action::Focus { target: deep() })
        .await
        .unwrap();
    browser
        .perform(
            &id,
            Action::Press {
                key: "Enter".to_owned(),
                target: Some(deep()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        fake.last("press"),
        json!({"action": "press", "key": "Enter"})
    );
    let text = browser
        .perform(&id, Action::GetText { target: deep() })
        .await
        .unwrap();
    assert_eq!(text.value, json!("LOG IN"));
    let visible = browser
        .perform(&id, Action::IsVisible { target: deep() })
        .await
        .unwrap();
    assert_eq!(visible.value, json!(true));
    assert_eq!(ops(&fake), ["find", "focus", "focus", "text", "visible"]);
    assert!(mouse(&fake).is_empty(), "a focus is no press");
}
