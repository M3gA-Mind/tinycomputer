//! Browser results and errors as the envelope decision loops read.

use serde_json::Value;
use tinycomputer_bus::{DesktopError, DesktopResponse};

use crate::error::{Error, Result};

/// A browser result as the envelope decision loops read.
pub(super) fn reply(command: &str, result: Result<Value>) -> DesktopResponse {
    match result {
        Ok(data) => DesktopResponse::ok(command, data),
        Err(error) => failure(command, &error),
    }
}

/// A browser error as an envelope failure whose code is the error's wire
/// name in `SCREAMING_SNAKE_CASE`, such as `STALE_REF`.
/// Whether a click was refused because another element covers its target.
pub(super) fn covered(reply: &DesktopResponse) -> bool {
    reply.error.as_ref().is_some_and(|error| {
        error.code != "ACTION_REFUSED" && error.message.contains("is covered by")
    })
}

pub(super) fn failure(command: &str, error: &Error) -> DesktopResponse {
    let name = error
        .wire_name()
        .rsplit('.')
        .next()
        .unwrap_or("ModuleFailed");
    let mut code = String::with_capacity(name.len() + 4);
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            code.push('_');
        }
        code.push(character.to_ascii_uppercase());
    }
    DesktopResponse::err(command, DesktopError::new(code, error.to_string()))
}

/// The refusal for text aimed at an element that does not take it.
pub(super) fn not_a_text_field() -> DesktopResponse {
    DesktopResponse::err(
        "type-text",
        DesktopError::new(
            "NOT_A_TEXT_FIELD",
            "the element does not take typed text: focusing it reaches no input, text area, or editable region",
        ),
    )
}

#[cfg(test)]
mod tests {
    use tinycomputer_bus::{DesktopError, DesktopResponse};

    use super::covered;

    #[test]
    fn a_host_policy_refusal_is_not_retried_as_a_cover() {
        let refused = DesktopResponse::err(
            "click",
            DesktopError::new(
                "ACTION_REFUSED",
                "Refused: the target is covered by policy.",
            ),
        );
        assert!(!covered(&refused));

        let covered_by_page = DesktopResponse::err(
            "click",
            DesktopError::new("NOT_ACTIONABLE", "The element is covered by a banner."),
        );
        assert!(covered(&covered_by_page));
    }
}
