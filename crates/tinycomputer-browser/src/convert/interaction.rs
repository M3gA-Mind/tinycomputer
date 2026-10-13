//! One interaction on the page, through a ref, a selector, or a semantic locator.

use serde_json::{Value, json};
use tinycomputer_bus::browser::{Action, LocateBy, Locator, ScrollDirection, Target, WaitState};

use super::TO_THE_END;
use crate::error::{Error, Result};

/// One interaction.
///
/// # Errors
///
/// [`Error::InvalidInput`] when the action names a locator the engine cannot
/// combine with it, or carries an empty argument.
pub(crate) fn action(action: &Action, default_timeout_ms: u64) -> Result<Value> {
    match action {
        Action::Scroll {
            direction,
            pixels,
            target,
        } => scroll(*direction, *pixels, target.as_ref()),
        Action::WaitFor {
            target,
            text,
            state,
            ms,
            timeout_ms,
        } => wait_for(
            target.as_ref(),
            text.as_deref(),
            *state,
            *ms,
            timeout_ms.unwrap_or(default_timeout_ms),
        ),
        Action::Back => Ok(json!({"action": "back"})),
        Action::Forward => Ok(json!({"action": "forward"})),
        Action::Reload => Ok(json!({"action": "reload"})),
        // A press goes wherever the focus is: with a target, `Browser::perform`
        // first puts the focus there and checks it landed.
        Action::Press { key, .. } => {
            if key.trim().is_empty() {
                return Err(Error::invalid_input("press needs a key"));
            }
            Ok(json!({"action": "press", "key": key}))
        }
        Action::Type {
            target,
            text,
            delay_ms,
        } => typing(target.as_ref(), text, *delay_ms),
        element => on_element(element),
    }
}

/// An action on one element, through a semantic locator where the engine
/// offers one.
fn on_element(action: &Action) -> Result<Value> {
    Ok(match action {
        Action::Click { target, new_tab } => match target {
            Target::Locator { value } => semantic(value, "click", None)?,
            other => {
                json!({"action": "click", "selector": selector(other, "click")?, "newTab": new_tab})
            }
        },
        Action::DoubleClick { target } => {
            json!({"action": "dblclick", "selector": selector(target, "double_click")?})
        }
        Action::Hover { target } => match target {
            Target::Locator { value } => semantic(value, "hover", None)?,
            other => json!({"action": "hover", "selector": selector(other, "hover")?}),
        },
        Action::Focus { target } => {
            json!({"action": "focus", "selector": selector(target, "focus")?})
        }
        Action::Fill { target, value } => match target {
            Target::Locator { value: locator } => semantic(locator, "fill", Some(value))?,
            other => {
                json!({"action": "fill", "selector": selector(other, "fill")?, "value": value})
            }
        },
        Action::Select { target, values } => {
            json!({"action": "select", "selector": selector(target, "select")?, "values": values})
        }
        Action::Check { target, checked } => match (target, checked) {
            (Target::Locator { value }, true) => semantic(value, "check", None)?,
            (other, true) => json!({"action": "check", "selector": selector(other, "check")?}),
            (other, false) => {
                json!({"action": "uncheck", "selector": selector(other, "uncheck")?})
            }
        },
        Action::GetText { target } => match target {
            Target::Locator { value } => semantic(value, "text", None)?,
            other => json!({"action": "gettext", "selector": selector(other, "get_text")?}),
        },
        Action::GetAttribute { target, attribute } => json!({
            "action": "getattribute",
            "selector": selector(target, "get_attribute")?,
            "attribute": attribute,
        }),
        Action::IsVisible { target } => {
            json!({"action": "isvisible", "selector": selector(target, "is_visible")?})
        }
        Action::Scroll { .. }
        | Action::WaitFor { .. }
        | Action::Back
        | Action::Forward
        | Action::Reload
        | Action::Press { .. }
        | Action::Type { .. } => {
            return Err(Error::failed("not an element action"));
        }
    })
}

fn typing(target: Option<&Target>, text: &str, delay_ms: Option<u64>) -> Result<Value> {
    let Some(target) = target else {
        return Ok(json!({"action": "inserttext", "text": text}));
    };
    let mut command =
        json!({"action": "type", "selector": selector(target, "type")?, "text": text});
    if let Some(delay) = delay_ms {
        command["delay"] = json!(delay);
    }
    Ok(command)
}

fn scroll(
    direction: ScrollDirection,
    pixels: Option<u32>,
    target: Option<&Target>,
) -> Result<Value> {
    let (direction, amount) = match direction {
        ScrollDirection::Down => ("down", pixels.unwrap_or(300)),
        ScrollDirection::Up => ("up", pixels.unwrap_or(300)),
        ScrollDirection::Left => ("left", pixels.unwrap_or(300)),
        ScrollDirection::Right => ("right", pixels.unwrap_or(300)),
        ScrollDirection::Top => ("up", TO_THE_END),
        ScrollDirection::Bottom => ("down", TO_THE_END),
    };
    let mut command = json!({"action": "scroll", "direction": direction, "amount": amount});
    if let Some(target) = target {
        command["selector"] = json!(selector(target, "scroll")?);
    }
    Ok(command)
}

fn wait_for(
    target: Option<&Target>,
    text: Option<&str>,
    state: WaitState,
    ms: Option<u64>,
    timeout: u64,
) -> Result<Value> {
    match (text, target, ms) {
        (Some(text), _, _) => Ok(json!({"action": "wait", "text": text, "timeout": timeout})),
        (None, Some(target), _) => Ok(json!({
            "action": "wait",
            "selector": selector(target, "wait_for")?,
            "state": wait_state(state),
            "timeout": timeout,
        })),
        (None, None, Some(ms)) => Ok(json!({"action": "wait", "timeout": ms})),
        (None, None, None) => Err(Error::invalid_input("wait_for needs a target, text, or ms")),
    }
}

/// The engine selector for a ref or CSS target.
///
/// # Errors
///
/// [`Error::InvalidInput`] for a locator, which agent-browser only accepts on
/// its semantic commands (click, fill, check, hover, text).
pub(crate) fn selector(target: &Target, operation: &str) -> Result<String> {
    match target {
        Target::Ref { value } if !value.trim().is_empty() => {
            Ok(format!("@{}", value.trim_start_matches('@')))
        }
        Target::Selector { value } if !value.trim().is_empty() => Ok(value.clone()),
        Target::Ref { .. } | Target::Selector { .. } => Err(Error::invalid_input(format!(
            "{operation} needs a non-empty target"
        ))),
        Target::Locator { .. } => Err(Error::invalid_input(format!(
            "{operation} takes a ref or selector; locators work with click, fill, check, hover, and get_text"
        ))),
    }
}

/// A semantic-locator command performing `subaction` on the match.
///
/// # Errors
///
/// [`Error::InvalidInput`] for an empty value or a non-first match index.
fn semantic(locator: &Locator, subaction: &str, value: Option<&String>) -> Result<Value> {
    if locator.value.trim().is_empty() {
        return Err(Error::invalid_input("a locator needs a value"));
    }
    if locator.index != 0 {
        return Err(Error::invalid_input(
            "locator index is not supported; take a snapshot and use a ref",
        ));
    }
    let (action, key) = match locator.by {
        LocateBy::Role => ("getbyrole", "role"),
        LocateBy::Text => ("getbytext", "text"),
        LocateBy::Label => ("getbylabel", "label"),
        LocateBy::Placeholder => ("getbyplaceholder", "placeholder"),
        LocateBy::TestId => ("getbytestid", "testId"),
        LocateBy::AltText => ("getbyalttext", "text"),
        LocateBy::Title => ("getbytitle", "text"),
    };
    let mut command = json!({
        "action": action,
        key: locator.value,
        "exact": locator.exact,
        "subaction": subaction,
    });
    if let Some(name) = &locator.name {
        command["name"] = json!(name);
    }
    if let Some(value) = value {
        command["value"] = json!(value);
    }
    Ok(command)
}

fn wait_state(state: WaitState) -> &'static str {
    match state {
        WaitState::Attached => "attached",
        WaitState::Detached => "detached",
        WaitState::Visible => "visible",
        WaitState::Hidden => "hidden",
    }
}
