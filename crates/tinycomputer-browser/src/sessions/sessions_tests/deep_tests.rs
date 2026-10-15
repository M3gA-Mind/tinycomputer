//! Tests for deep controls on a session: the look after each snapshot, and
//! pressing, hovering, and refusing the `d` refs it hands out, over a
//! scripted engine.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;
use tinycomputer_bus::browser::{Action, NavigateRequest, SnapshotRequest, Target};

use super::deep_page::{TREE, click, mouse, op, ops, options, page, shown};
use super::open;
use crate::error::Error;
use crate::fake::{Fake, failure, ok};

#[tokio::test]
async fn a_snapshot_lists_the_deep_controls_with_refs_of_their_own() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-snapshot").await;
    let shot = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_eq!(
        shot.tree,
        "- banner\n  - clickable \"LOG IN\" [ref=d1]\n- button \"Book\" [ref=e1]\n\
         - group \"Controls inside web components\"\n  - clickable [ref=d2]"
    );
    let ids: Vec<_> = shot
        .refs
        .iter()
        .map(|element| element.id.as_str())
        .collect();
    assert_eq!(ids, ["e1", "d1", "d2"]);
    // The next look keeps the numbers handed out on what still shows the
    // same words, and mints past them.
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let looks = options(&fake, "find");
    assert_eq!(looks[0]["keep"], json!([]));
    assert_eq!(
        looks[1]["keep"],
        json!([{"n": 1, "text": "LOG IN"}, {"n": 2, "text": ""}])
    );
    assert_eq!(looks[1]["next"], json!(3));
}

#[tokio::test]
async fn a_page_with_no_deep_controls_is_snapshotted_as_the_engine_gave_it() {
    // The default engine answers every script with 42: no look's reply.
    let fake = Fake::scripted(|command| {
        (command["action"] == "snapshot").then(|| {
            ok(&json!({
                "snapshot": TREE,
                "refs": {"e1": {"role": "button", "name": "Book"}},
            }))
        })
    });
    let (browser, id) = open(&fake, "deep-none").await;
    let shot = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_eq!(shot.tree, TREE);
    assert_eq!(shot.refs.len(), 1);
    // A look that fails leaves it as it was too.
    let failing = Fake::scripted(|command| {
        op(command).map(|_| failure("Evaluation error: the page is gone"))
    });
    let (browser, id) = open(&failing, "deep-failing").await;
    let shot = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_eq!(shot.tree, "- button \"Search\" [ref=e1]");
    assert_eq!(ops(&failing), ["find"]);
}

#[tokio::test]
async fn a_click_presses_the_middle_of_the_element_with_the_pointer() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-click").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let outcome = browser.perform(&id, click("@d1")).await.unwrap();
    assert!(outcome.value.is_null());
    assert_eq!(outcome.page.url, "https://flights.test/");
    // Aimed at the element still showing its words, guarded, pressed, and
    // the guard read.
    assert_eq!(ops(&fake), ["find", "aim", "pressed"]);
    let aimed = &options(&fake, "aim")[0];
    assert_eq!(aimed["words"], "LOG IN");
    assert_eq!(
        (&aimed["guard"], &aimed["field"]),
        (&json!(true), &json!(false))
    );
    assert_eq!(
        mouse(&fake),
        [
            ("mouseMoved".to_owned(), 40.0, 12.0),
            ("mousePressed".to_owned(), 40.0, 12.0),
            ("mouseReleased".to_owned(), 40.0, 12.0),
        ]
    );
    // Never through agent-browser, which cannot resolve it.
    assert!(!fake.actions().iter().any(|action| action == "click"));
}

#[tokio::test]
async fn a_hover_only_moves_the_pointer_there() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-hover").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    browser
        .perform(
            &id,
            Action::Hover {
                target: Target::reference("d2"),
            },
        )
        .await
        .unwrap();
    assert_eq!(mouse(&fake), [("mouseMoved".to_owned(), 40.0, 12.0)]);
    assert_eq!(
        options(&fake, "aim")[0]["guard"],
        false,
        "a hover is no press"
    );
    assert_eq!(ops(&fake), ["find", "aim"]);
}

#[tokio::test]
async fn a_deep_ref_not_handed_out_or_gone_is_stale() {
    let fake = page(|_| json!({"gone": true}));
    let (browser, id) = open(&fake, "deep-stale").await;
    // Before any snapshot, no deep ref is handed out: nothing is sent.
    let before = browser.perform(&id, click("d1")).await.unwrap_err();
    assert!(matches!(&before, Error::StaleRef { reference } if reference == "d1"));
    assert_eq!(ops(&fake).len(), 0);
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert!(matches!(
        browser.perform(&id, click("d9")).await,
        Err(Error::StaleRef { reference }) if reference == "d9"
    ));
    // Handed out, but the page re-drew it.
    assert!(matches!(
        browser.perform(&id, click("d1")).await,
        Err(Error::StaleRef { reference }) if reference == "d1"
    ));
    assert!(mouse(&fake).is_empty(), "nothing was pressed");
}

#[tokio::test]
async fn a_covered_or_unseen_element_is_not_pressed() {
    let fake = page(|op| match op {
        "aim" => json!({"x": 40.0, "y": 12.0, "cover": "div.mask.dim<script>"}),
        _ => json!({"gone": true}),
    });
    let (browser, id) = open(&fake, "deep-covered").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let covered = browser.perform(&id, click("d1")).await.unwrap_err();
    assert!(
        matches!(&covered, Error::NotActionable { reason }
            if reason == "@d1 is covered by <div.mask.dimscript> at its middle, so a press would land on that instead; dismiss or close what covers it first"),
        "{covered}"
    );
    let unseen = page(|op| match op {
        "aim" => json!({"unseen": true}),
        _ => json!({"gone": true}),
    });
    let unmeasured = page(|op| match op {
        "aim" => json!({"cover": "div"}),
        _ => json!({"gone": true}),
    });
    let (browser, id) = open(&unmeasured, "deep-unmeasured").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert!(matches!(
        browser.perform(&id, click("d1")).await,
        Err(Error::NotActionable { reason }) if reason == "@d1 could not be measured"
    ));
    let (browser, id) = open(&unseen, "deep-unseen").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert!(matches!(
        browser.perform(&id, click("d2")).await,
        Err(Error::NotActionable { reason }) if reason.contains("shows nowhere")
    ));
    assert!(mouse(&fake).is_empty() && mouse(&unseen).is_empty());
}

#[tokio::test]
async fn what_a_deep_ref_cannot_take_is_refused_before_the_page_is_touched() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-refused").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let sent = fake.sent().len();
    for action in [
        Action::Check {
            target: Target::reference("d1"),
            checked: true,
        },
        Action::Click {
            target: Target::reference("d1"),
            new_tab: true,
        },
    ] {
        assert!(matches!(
            browser.perform(&id, action).await,
            Err(Error::InvalidInput { .. })
        ));
    }
    assert_eq!(fake.sent().len(), sent, "nothing was sent");
    // Asked directly, the session refuses it too, and sends nothing.
    let session = browser.session(&id).unwrap();
    let mut session = session.lock().await;
    let refused = session.act_deep(1, &Action::Back).await.unwrap_err();
    assert!(matches!(refused, Error::InvalidInput { .. }));
    assert_eq!(fake.sent().len(), sent, "nothing was sent");
}

#[tokio::test]
async fn a_press_stopped_on_its_way_is_refused_and_one_that_left_the_page_is_not() {
    let moved = page(|op| match op {
        "pressed" => json!({"missed": "div.popup"}),
        other => shown(other),
    });
    let (browser, id) = open(&moved, "deep-missed").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let refused = browser.perform(&id, click("d1")).await.unwrap_err();
    assert!(
        matches!(&refused, Error::NotActionable { reason }
            if reason == "@d1 moved, or <div.popup> came over it, as it was pressed, so the press was stopped; take a fresh snapshot and retry"),
        "{refused}"
    );
    // A press whose page went away with it pressed something: it went.
    let left = Fake::scripted(|command| {
        if command["action"] == "snapshot" {
            return Some(ok(&json!({"snapshot": TREE, "refs": {}})));
        }
        Some(match op(command)?.as_str() {
            "pressed" => failure("Execution context was destroyed."),
            "find" => ok(&json!({"result": {
                "found": [{"n": 1, "kind": "clickable", "text": "LOG IN"}],
                "next": 2,
            }})),
            other => ok(&json!({ "result": shown(other) })),
        })
    });
    let (browser, id) = open(&left, "deep-left").await;
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert!(browser.perform(&id, click("d1")).await.is_ok());
}

#[tokio::test]
async fn an_element_that_will_not_hold_still_or_shows_another_control_is_not_pressed() {
    for (aimed, said) in [
        (json!({"moving": true}), "would not hold still"),
        (
            json!({"nested": "button.details"}),
            "shows another control everywhere a press could reach it (<button.details>)",
        ),
    ] {
        let fake = page(move |op| {
            if op == "aim" {
                aimed.clone()
            } else {
                shown(op)
            }
        });
        let (browser, id) = open(&fake, "deep-unsteady").await;
        browser
            .snapshot(&id, SnapshotRequest::default())
            .await
            .unwrap();
        let refused = browser.perform(&id, click("d1")).await.unwrap_err();
        assert!(
            matches!(&refused, Error::NotActionable { reason } if reason.contains(said)),
            "{refused}"
        );
        assert_eq!(mouse(&fake).len(), 0, "nothing was pressed");
    }
}

#[tokio::test]
async fn while_agent_browser_reads_a_frame_a_snapshot_has_no_look() {
    let fake = page(shown);
    let (browser, id) = open(&fake, "deep-frame").await;
    browser
        .command(&id, json!({"action": "frame", "selector": "#payment"}))
        .await
        .unwrap();
    let framed = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_eq!(framed.tree, TREE);
    assert_eq!(
        ops(&fake).len(),
        0,
        "the script never sees a frame's document"
    );
    browser
        .command(&id, json!({"action": "mainframe"}))
        .await
        .unwrap();
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_eq!(ops(&fake), ["find"]);
    // A page loaded anew is read from its top document too.
    browser
        .command(&id, json!({"action": "frame", "selector": "#payment"}))
        .await
        .unwrap();
    browser
        .navigate(&id, NavigateRequest::new("https://flights.test/pay"))
        .await
        .unwrap();
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_eq!(ops(&fake), ["find", "find"]);
}

#[tokio::test]
async fn a_look_that_never_replies_hands_out_none_and_its_numbers_never() {
    // The first look is never answered, as on a page being replaced; it may
    // still have marked elements before its time ran out.
    let stalled = Arc::new(AtomicBool::new(false));
    let fake = page(shown).stalling(move |command| {
        op(command).as_deref() == Some("find") && !stalled.swap(true, Ordering::SeqCst)
    });
    let (browser, id) = open(&fake, "deep-lost").await;
    let lost = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_eq!(lost.tree, TREE);
    assert_eq!(lost.refs.len(), 1);
    browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    let looks = options(&fake, "find");
    assert_eq!(looks[0]["next"], 1);
    assert_eq!(
        looks[1]["next"], 41,
        "no number the lost look had is handed out"
    );
    assert_eq!(looks[1]["keep"], json!([]));
}
