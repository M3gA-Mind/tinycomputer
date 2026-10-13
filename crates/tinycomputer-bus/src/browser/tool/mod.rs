//! The agent-facing `browser` tool: its recorded parameter schema and the
//! pure parsing of its arguments into the module's [`Action`] vocabulary.
//!
//! A host registers one model-visible tool, [`TOOL_NAME`], whose JSON schema
//! is [`PARAMETERS_JSON`]. It deserializes the model's arguments into
//! [`ToolArgs`], applies its own approval and allow-list policy, and either
//! converts them with [`ToolArgs::to_action`] for `BrowserPerform` or reads the
//! task fields for `StartTask`/`ContinueTask`. Nothing here performs I/O.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Action, LocateBy, Locator, ScrollDirection, Target, WaitState};

/// The model-visible name of the browser tool.
pub const TOOL_NAME: &str = "browser";

/// The tool's parameter schema as recorded JSON (draft-07 object schema).
///
/// Held as text so a host parses it once; [`parameters_schema`] does that.
pub const PARAMETERS_JSON: &str = include_str!("parameters.json");

/// The tool's parameter schema, parsed from [`PARAMETERS_JSON`].
///
/// # Panics
///
/// Never in practice: the recorded text is valid JSON, which a test pins.
#[must_use]
pub fn parameters_schema() -> serde_json::Value {
    serde_json::from_str(PARAMETERS_JSON).unwrap_or(serde_json::Value::Null)
}

/// Every value of the schema's `action` enum, in schema order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolAction {
    /// Open a browser session on an HTTPS URL.
    Open,
    /// Snapshot the accessibility tree.
    Snapshot,
    /// Read the page as text.
    ReadPage,
    /// Click an element.
    Click,
    /// Fill a field.
    Fill,
    /// Type text.
    Type,
    /// Read an element's text.
    GetText,
    /// Read the page title.
    GetTitle,
    /// Read the page URL.
    GetUrl,
    /// Wait for an element, text, or a delay.
    Wait,
    /// Press a key.
    Press,
    /// Hover an element.
    Hover,
    /// Scroll the page.
    Scroll,
    /// Whether an element is visible.
    IsVisible,
    /// Find an element by role, text, label, placeholder, or test id, and act on it.
    Find,
    /// Start a goal-driven task.
    Task,
    /// Continue a paused task.
    TaskContinue,
    /// Cancel a task.
    TaskCancel,
    /// Confirm the pending action the host is holding.
    ConfirmPending,
    /// List retained downloads.
    ListDownloads,
    /// Wait for a download to finish.
    WaitDownload,
    /// Close the session.
    Close,
}

impl ToolAction {
    /// The action's wire name, as in the schema's `action` enum.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Snapshot => "snapshot",
            Self::ReadPage => "read_page",
            Self::Click => "click",
            Self::Fill => "fill",
            Self::Type => "type",
            Self::GetText => "get_text",
            Self::GetTitle => "get_title",
            Self::GetUrl => "get_url",
            Self::Wait => "wait",
            Self::Press => "press",
            Self::Hover => "hover",
            Self::Scroll => "scroll",
            Self::IsVisible => "is_visible",
            Self::Find => "find",
            Self::Task => "task",
            Self::TaskContinue => "task_continue",
            Self::TaskCancel => "task_cancel",
            Self::ConfirmPending => "confirm_pending",
            Self::ListDownloads => "list_downloads",
            Self::WaitDownload => "wait_download",
            Self::Close => "close",
        }
    }
}

/// The model's arguments to the browser tool: every property of the schema.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolArgs {
    /// What to do.
    pub action: Option<ToolAction>,
    /// Starting HTTPS URL for `open`, or an optional one for `task`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// CSS selector or `@ref` of the element to act on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Value to fill, or the locator value for `find`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Text to type, or text to wait for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Key to press.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Scroll direction: `up`, `down`, `left`, `right`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// Scroll distance in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixels: Option<u64>,
    /// Fixed delay for `wait`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// Timeout for `wait` and `wait_download`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Restrict a snapshot to interactive elements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive_only: Option<bool>,
    /// Compact snapshot output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    /// Snapshot depth limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u64>,
    /// Locator kind for `find`: `role`, `text`, `label`, `placeholder`, `testid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// What `find` does with the element: `click`, `fill`, `text`, `hover`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub find_action: Option<String>,
    /// Value `find` fills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_value: Option<String>,
    /// The goal of a `task`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Named text inputs for a `task`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<BTreeMap<String, String>>,
    /// Task id from `task`, for `task_continue` and `task_cancel`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// A `TinyComputer` flow to run instead of planning one from `goal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<serde_json::Value>,
    /// Free-text answer for a paused task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Token returned with the exact pending action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Why tool arguments could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ToolArgError {
    /// A parameter the action needs is absent or blank.
    #[error("Missing '{0}' parameter")]
    Missing(&'static str),
    /// A parameter holds a value outside its allowed set.
    #[error("Invalid {what}: {value}")]
    Invalid {
        /// What kind of value it was (`direction`, `locator`, `find action`).
        what: &'static str,
        /// The offending value.
        value: String,
    },
    /// The action is not a single `BrowserPerform` action.
    #[error("Unsupported browser action: {0}")]
    Unsupported(&'static str),
}

impl ToolArgs {
    /// The text of `field`, or [`ToolArgError::Missing`] when absent or blank.
    ///
    /// # Errors
    ///
    /// [`ToolArgError::Missing`] naming `key`.
    pub fn required<'a>(
        field: &'a Option<String>,
        key: &'static str,
    ) -> Result<&'a str, ToolArgError> {
        field
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or(ToolArgError::Missing(key))
    }

    /// The `action`, or [`ToolArgError::Missing`] when absent.
    ///
    /// # Errors
    ///
    /// [`ToolArgError::Missing`] naming `action`.
    pub fn action(&self) -> Result<ToolAction, ToolArgError> {
        self.action.ok_or(ToolArgError::Missing("action"))
    }

    /// The module [`Action`] a page-level tool action performs.
    ///
    /// Only actions that map to one `BrowserPerform` action convert; session
    /// and task actions are [`ToolArgError::Unsupported`].
    ///
    /// # Errors
    ///
    /// A [`ToolArgError`] for a missing or out-of-range parameter, or an
    /// action that is not a single page action.
    pub fn to_action(&self) -> Result<Action, ToolArgError> {
        let target = || Self::required(&self.selector, "selector").map(Target::parse);
        Ok(match self.action()? {
            ToolAction::Click => Action::Click {
                target: target()?,
                new_tab: false,
            },
            ToolAction::Fill => Action::Fill {
                target: target()?,
                value: Self::required(&self.value, "value")?.into(),
            },
            ToolAction::Type => Action::Type {
                target: self.selector.as_deref().map(Target::parse),
                text: Self::required(&self.text, "text")?.into(),
                delay_ms: None,
            },
            ToolAction::GetText => Action::GetText { target: target()? },
            ToolAction::IsVisible => Action::IsVisible { target: target()? },
            ToolAction::Hover => Action::Hover { target: target()? },
            ToolAction::Press => Action::Press {
                key: Self::required(&self.key, "key")?.into(),
            },
            ToolAction::Scroll => Action::Scroll {
                direction: match Self::required(&self.direction, "direction")? {
                    "up" => ScrollDirection::Up,
                    "down" => ScrollDirection::Down,
                    "left" => ScrollDirection::Left,
                    "right" => ScrollDirection::Right,
                    x => return Err(invalid("direction", x)),
                },
                pixels: self.pixels.and_then(|v| u32::try_from(v).ok()),
                target: None,
            },
            ToolAction::Wait => Action::WaitFor {
                target: self.selector.as_deref().map(Target::parse),
                text: self.text.clone(),
                state: WaitState::Visible,
                ms: self.ms,
                timeout_ms: self.timeout_ms,
            },
            ToolAction::Find => {
                let by = match Self::required(&self.by, "by")? {
                    "role" => LocateBy::Role,
                    "text" => LocateBy::Text,
                    "label" => LocateBy::Label,
                    "placeholder" => LocateBy::Placeholder,
                    "testid" => LocateBy::TestId,
                    x => return Err(invalid("locator", x)),
                };
                let target =
                    Target::locator(Locator::new(by, Self::required(&self.value, "value")?));
                match Self::required(&self.find_action, "find_action")? {
                    "click" => Action::Click {
                        target,
                        new_tab: false,
                    },
                    "fill" => Action::Fill {
                        target,
                        value: Self::required(&self.fill_value, "fill_value")?.into(),
                    },
                    "text" => Action::GetText { target },
                    "hover" => Action::Hover { target },
                    x => return Err(invalid("find action", x)),
                }
            }
            other => return Err(ToolArgError::Unsupported(other.as_str())),
        })
    }
}

fn invalid(what: &'static str, value: &str) -> ToolArgError {
    ToolArgError::Invalid {
        what,
        value: value.to_owned(),
    }
}

#[cfg(test)]
mod tool_tests;
