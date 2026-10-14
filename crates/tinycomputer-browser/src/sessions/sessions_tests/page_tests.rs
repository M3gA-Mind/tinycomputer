//! Tests for the calls on a session's page: navigation, snapshots, actions,
//! reading, evaluation, and raw commands.

use serde_json::{Value, json};
use tinycomputer_bus::browser::{
    Action, EvaluateRequest, LocateBy, Locator, NavigateRequest, ReadFormat, ReadRequest,
    SnapshotRequest, Target,
};

use super::open;
use crate::error::Error;
use crate::fake::{Fake, failure, ok};

#[tokio::test]
async fn navigation_reports_the_page_and_refusals_are_typed() {
    let fake = Fake::scripted(|command| {
        (command["url"] == "https://evil.test/")
            .then(|| failure("Domain 'evil.test' is not in the allowed domains list"))
    });
    let (browser, id) = open(&fake, "navigate").await;
    let page = browser
        .navigate(&id, NavigateRequest::new("https://flights.test/search"))
        .await
        .unwrap();
    assert_eq!(page.url, "https://flights.test/search");
    assert_eq!(page.title, "Loaded");
    assert_eq!(
        browser.list_sessions().await.unwrap()[0].url,
        "https://flights.test/search"
    );
    assert!(matches!(
        browser.navigate(&id, NavigateRequest::new("https://evil.test/")).await,
        Err(Error::BlockedByPolicy { url }) if url == "evil.test"
    ));
    assert!(matches!(
        browser.navigate(&id, NavigateRequest::new("")).await,
        Err(Error::InvalidInput { .. })
    ));
}

#[tokio::test]
async fn snapshots_count_up_and_carry_the_page() {
    let fake = Fake::new();
    let (browser, id) = open(&fake, "snapshot").await;
    let first = browser
        .snapshot(&id, SnapshotRequest::interactive())
        .await
        .unwrap();
    let second = browser
        .snapshot(&id, SnapshotRequest::default())
        .await
        .unwrap();
    assert_eq!((first.sequence, second.sequence), (1, 2));
    assert_eq!(first.refs[0].id, "e1");
    assert_eq!(first.title, "Flights");
    assert_eq!(fake.last("snapshot")["interactive"], false);
}

#[tokio::test]
async fn actions_return_read_values_and_the_page_they_left() {
    let fake = Fake::new();
    let (browser, id) = open(&fake, "perform").await;
    let text = browser
        .perform(
            &id,
            Action::GetText {
                target: Target::reference("e1"),
            },
        )
        .await
        .unwrap();
    assert_eq!(text.value, json!("IndiGo ₹6,840"));
    assert_eq!(text.page.url, "https://flights.test/");
    let attribute = browser
        .perform(
            &id,
            Action::GetAttribute {
                target: Target::reference("e1"),
                attribute: "href".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(attribute.value, json!("/book"));
    let visible = browser
        .perform(
            &id,
            Action::IsVisible {
                target: Target::reference("e1"),
            },
        )
        .await
        .unwrap();
    assert_eq!(visible.value, json!(true));
    let clicked = browser
        .perform(
            &id,
            Action::Click {
                target: Target::locator(Locator::new(LocateBy::Role, "button")),
                new_tab: false,
            },
        )
        .await
        .unwrap();
    assert!(clicked.value.is_null());
    assert_eq!(clicked.matched.as_deref(), Some("Role \"button\""));
    let filled = browser
        .perform(
            &id,
            Action::Fill {
                target: Target::locator(Locator::new(LocateBy::Label, "From")),
                value: "DEL".to_owned(),
            },
        )
        .await
        .unwrap();
    assert!(filled.matched.is_some());
    let pressed = browser
        .perform(
            &id,
            Action::Press {
                key: "Enter".to_owned(),
                target: None,
            },
        )
        .await
        .unwrap();
    assert!(pressed.matched.is_none());
    assert_eq!(
        browser
            .perform(
                &id,
                Action::WaitFor {
                    target: None,
                    text: Some("Results".to_owned()),
                    state: tinycomputer_bus::browser::WaitState::Visible,
                    ms: None,
                    timeout_ms: None,
                },
            )
            .await
            .unwrap()
            .value,
        Value::Null
    );
    assert_eq!(fake.last("wait")["timeout"], 30_000);
}

#[tokio::test]
async fn a_stale_ref_and_an_inexpressible_action_are_typed_failures() {
    let fake = Fake::scripted(|command| {
        (command["action"] == "click").then(|| failure("Unknown ref: e7"))
    });
    let (browser, id) = open(&fake, "stale").await;
    assert!(matches!(
        browser
            .perform(
                &id,
                Action::Click {
                    target: Target::reference("e7"),
                    new_tab: false,
                },
            )
            .await,
        Err(Error::StaleRef { reference }) if reference == "e7"
    ));
    assert!(matches!(
        browser
            .perform(
                &id,
                Action::Focus {
                    target: Target::locator(Locator::new(LocateBy::Text, "Book")),
                },
            )
            .await,
        Err(Error::InvalidInput { .. })
    ));
}

#[tokio::test]
async fn reading_a_page_truncates_and_reports_its_format() {
    let fake = Fake::new();
    let (browser, id) = open(&fake, "read").await;
    let markdown = browser
        .read_page(
            &id,
            ReadRequest {
                max_chars: 5,
                ..ReadRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(markdown.content, "# Fli");
    assert!(markdown.truncated);
    assert_eq!(markdown.format, ReadFormat::Markdown);
    let html = browser
        .read_page(
            &id,
            ReadRequest {
                format: ReadFormat::Html,
                ..ReadRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(html.content, "<h1>Flights</h1>");
    assert!(!html.truncated);
    let scoped = browser
        .read_page(
            &id,
            ReadRequest {
                selector: Some("main".to_owned()),
                ..ReadRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(scoped.format, ReadFormat::Text);
    assert_eq!(scoped.content, "IndiGo ₹6,840");
}

#[tokio::test]
async fn evaluation_returns_the_scripts_value() {
    let fake = Fake::new();
    let (browser, id) = open(&fake, "evaluate").await;
    assert_eq!(
        browser
            .evaluate(&id, EvaluateRequest::new("6 * 7"))
            .await
            .unwrap(),
        json!(42)
    );
    assert!(matches!(
        browser.evaluate(&id, EvaluateRequest::new(" ")).await,
        Err(Error::InvalidInput { .. })
    ));
}

#[tokio::test]
async fn a_raw_command_needs_an_action_and_returns_its_data() {
    let fake = Fake::scripted(|command| {
        (command["action"] == "inputvalue").then(|| ok(&json!({"value": "Delhi"})))
    });
    let (browser, id) = open(&fake, "command").await;
    assert_eq!(
        browser
            .command(&id, json!({"action": "inputvalue", "selector": "@e2"}))
            .await
            .unwrap(),
        json!({"value": "Delhi"})
    );
    assert!(matches!(
        browser.command(&id, json!({"selector": "@e2"})).await,
        Err(Error::InvalidInput { .. })
    ));
}
