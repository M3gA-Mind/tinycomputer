//! Public error API preserves its established wire and display forms.
#![allow(
    clippy::expect_used,
    reason = "tests assert serialization and decoding"
)]

use super::*;

#[test]
fn accessibility_errors_keep_their_serde_and_display_forms() {
    let error = Error::FocusChanged {
        expected: "Editor".into(),
        actual: "Browser".into(),
    };
    assert_eq!(
        serde_json::to_value(&error).expect("serialize"),
        serde_json::json!({"FocusChanged":{"expected":"Editor","actual":"Browser"}})
    );
    let decoded: Error = serde_json::from_value(
        serde_json::json!({"FocusChanged":{"expected":"Editor","actual":"Browser"}}),
    )
    .expect("decode");
    assert_eq!(
        decoded.to_string(),
        "focus shifted from 'Editor' to 'Browser', aborting insertion"
    );
}
