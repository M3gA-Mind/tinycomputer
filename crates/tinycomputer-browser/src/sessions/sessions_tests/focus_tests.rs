//! Tests that keys go only where an action points: the focus is taken off
//! whatever held it, given to the target, and checked to be there, or the
//! action is refused with nothing typed or pressed.

use serde_json::json;
use tinycomputer_bus::browser::{Action, LocateBy, Locator, Target};

use super::open;
use crate::error::Error;
use crate::fake::{Fake, PageFocus, focus_mark};

/// A page whose focus, once an element is given it, is `focus`.
fn page(focus: PageFocus) -> Fake {
    Fake::new().with_focus(focus)
}

/// The commands that move or check the focus, or send keys, in order:
/// `blur`, `mark`, and `unmark` for the three page scripts.
fn steps(fake: &Fake) -> Vec<String> {
    fake.sent()
        .iter()
        .filter_map(|command| {
            let action = command["action"].as_str().unwrap_or_default();
            let script = command["script"].as_str().unwrap_or_default();
            match action {
                "evaluate" if script.contains("element.blur()") => Some("blur".to_owned()),
                "evaluate" if script.contains("setAttribute") => Some("mark".to_owned()),
                "evaluate" if script.contains("removeAttribute") => Some("unmark".to_owned()),
                "focus" | "getattribute" | "type" | "inserttext" | "press" | "fill"
                | "getbylabel" => Some(action.to_owned()),
                _ => None,
            }
        })
        .collect()
}

/// Every mark value a marking script put down, in order.
fn marks(fake: &Fake) -> Vec<String> {
    fake.sent()
        .iter()
        .filter_map(|command| focus_mark(command["script"].as_str()?))
        .collect()
}

fn typing(target: Option<Target>) -> Action {
    Action::Type {
        target,
        text: "4242 4242 4242 4242".to_owned(),
        delay_ms: None,
    }
}

#[tokio::test]
async fn typing_into_a_target_gives_it_the_focus_and_checks_it_first() {
    let fake = page(PageFocus::OnTarget);
    let (browser, id) = open(&fake, "focus-typed").await;
    browser
        .perform(&id, typing(Some(Target::reference("e3"))))
        .await
        .unwrap();
    assert_eq!(
        steps(&fake),
        ["blur", "focus", "mark", "getattribute", "unmark", "type"]
    );
    assert_eq!(fake.last("focus")["selector"], "@e3");
    assert_eq!(fake.last("getattribute")["selector"], "@e3");
    assert_eq!(fake.last("type")["selector"], "@e3");
    // The mark it read back is the one it put down.
    let unmark = fake
        .sent()
        .into_iter()
        .rev()
        .find(|command| {
            command["script"]
                .as_str()
                .is_some_and(|script| script.contains("removeAttribute"))
        })
        .unwrap();
    assert!(
        unmark["script"]
            .as_str()
            .unwrap()
            .ends_with(&format!("(\"{}\")", marks(&fake)[0]))
    );
}

#[tokio::test]
async fn text_meant_for_a_heading_never_reaches_the_field_that_had_the_focus() {
    let fake = page(PageFocus::Elsewhere("input"));
    let (browser, id) = open(&fake, "focus-heading").await;
    let refused = browser
        .perform(&id, typing(Some(Target::reference("e1"))))
        .await;
    assert!(
        matches!(&refused, Err(Error::FocusMissed { focused }) if focused == "input"),
        "{refused:?}"
    );
    // Nothing typed, and the marks came off.
    assert_eq!(
        steps(&fake),
        ["blur", "focus", "mark", "getattribute", "unmark"]
    );
}

#[tokio::test]
async fn a_target_that_takes_no_focus_is_refused_without_asking_for_the_mark() {
    let fake = page(PageFocus::Nowhere);
    let (browser, id) = open(&fake, "focus-nowhere").await;
    let refused = browser
        .perform(
            &id,
            Action::Fill {
                target: Target::reference("e2"),
                value: "Ada".to_owned(),
            },
        )
        .await;
    assert!(
        matches!(&refused, Err(Error::FocusMissed { focused }) if focused == "nothing"),
        "{refused:?}"
    );
    assert_eq!(steps(&fake), ["blur", "focus", "mark", "unmark"]);
}

#[tokio::test]
async fn the_focus_inside_a_cross_origin_frame_goes_ahead() {
    // A payment provider's card field: the page cannot see into its frame,
    // and with the focus taken off everything first, only the target can
    // have put it there.
    let fake = page(PageFocus::InAFrame);
    let (browser, id) = open(&fake, "focus-frame").await;
    browser
        .perform(
            &id,
            Action::Fill {
                target: Target::reference("e9"),
                value: "4242 4242 4242 4242".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(steps(&fake), ["blur", "focus", "mark", "unmark", "fill"]);
}

#[tokio::test]
async fn a_key_pressed_on_an_element_goes_only_to_it() {
    let pressing = Action::Press {
        key: "Enter".to_owned(),
        target: Some(Target::reference("e5")),
    };
    // A heading, with the order button holding the focus: no press.
    let fake = page(PageFocus::Elsewhere("button"));
    let (browser, id) = open(&fake, "press-refused").await;
    let refused = browser.perform(&id, pressing.clone()).await;
    assert!(
        matches!(&refused, Err(Error::FocusMissed { focused }) if focused == "button"),
        "{refused:?}"
    );
    assert!(!steps(&fake).contains(&"press".to_owned()));
    // The element itself: the key goes, to the focus it now holds.
    let fake = page(PageFocus::OnTarget);
    let (browser, id) = open(&fake, "press-checked").await;
    browser.perform(&id, pressing).await.unwrap();
    assert_eq!(
        steps(&fake),
        ["blur", "focus", "mark", "getattribute", "unmark", "press"]
    );
    assert_eq!(
        fake.last("press"),
        json!({"action": "press", "key": "Enter"})
    );
}

#[tokio::test]
async fn keys_without_a_target_go_where_the_focus_is_unchecked() {
    let fake = page(PageFocus::Elsewhere("input"));
    let (browser, id) = open(&fake, "focus-untargeted").await;
    browser
        .perform(
            &id,
            Action::Press {
                key: "Tab".to_owned(),
                target: None,
            },
        )
        .await
        .unwrap();
    browser.perform(&id, typing(None)).await.unwrap();
    assert_eq!(steps(&fake), ["press", "inserttext"]);
}

#[tokio::test]
async fn a_locator_fill_takes_the_focus_off_first_but_is_not_checked() {
    // Only the engine resolves a locator, so there is no selector to check;
    // with the focus off everything, its text lands in the field or nowhere.
    let fake = page(PageFocus::Elsewhere("input"));
    let (browser, id) = open(&fake, "focus-locator").await;
    browser
        .perform(
            &id,
            Action::Fill {
                target: Target::locator(Locator::new(LocateBy::Label, "Name")),
                value: "Ada".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(steps(&fake), ["blur", "getbylabel"]);
}

#[tokio::test]
async fn focusing_an_element_that_takes_none_is_refused() {
    let fake = page(PageFocus::Elsewhere("input"));
    let (browser, id) = open(&fake, "focus-action").await;
    assert!(matches!(
        browser
            .perform(
                &id,
                Action::Focus {
                    target: Target::reference("e1"),
                },
            )
            .await,
        Err(Error::FocusMissed { .. })
    ));
}

#[tokio::test]
async fn a_press_on_a_locator_is_refused_before_anything_is_sent() {
    let fake = page(PageFocus::OnTarget);
    let (browser, id) = open(&fake, "press-locator").await;
    let before = fake.sent().len();
    assert!(matches!(
        browser
            .perform(
                &id,
                Action::Press {
                    key: "Enter".to_owned(),
                    target: Some(Target::locator(Locator::new(LocateBy::Text, "Pay"))),
                },
            )
            .await,
        Err(Error::InvalidInput { .. })
    ));
    assert_eq!(fake.sent().len(), before);
}

#[tokio::test]
async fn a_failed_mark_read_is_reported_and_the_marks_still_come_off() {
    let fake = page(PageFocus::Stale);
    let (browser, id) = open(&fake, "focus-stale").await;
    assert!(matches!(
        browser
            .perform(&id, typing(Some(Target::reference("e3"))))
            .await,
        Err(Error::StaleRef { reference }) if reference == "e3"
    ));
    assert_eq!(
        steps(&fake),
        ["blur", "focus", "mark", "getattribute", "unmark"]
    );
}

#[tokio::test]
async fn where_the_focus_is_is_named_by_tag_only() {
    // Page text never rides in an error a model reads.
    let fake = page(PageFocus::Elsewhere("input onfocus=alert(1)"));
    let (browser, id) = open(&fake, "focus-named").await;
    let refused = browser
        .perform(&id, typing(Some(Target::reference("e1"))))
        .await;
    assert!(
        matches!(&refused, Err(Error::FocusMissed { focused }) if focused == "nothing"),
        "{refused:?}"
    );
}

#[tokio::test]
async fn every_check_uses_a_fresh_mark_a_page_cannot_guess() {
    let fake = page(PageFocus::OnTarget);
    let (browser, id) = open(&fake, "focus-marks").await;
    for _ in 0..2 {
        browser
            .perform(&id, typing(Some(Target::reference("e3"))))
            .await
            .unwrap();
    }
    let marks = marks(&fake);
    assert_eq!(marks.len(), 2);
    assert_ne!(marks[0], marks[1]);
    assert!(
        marks
            .iter()
            .all(|mark| mark.starts_with("tc-") && mark.len() == 19)
    );
}
