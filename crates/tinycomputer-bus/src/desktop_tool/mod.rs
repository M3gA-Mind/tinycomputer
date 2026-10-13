//! The agent-facing desktop tools: their names, descriptions, recorded
//! parameter schemas, and the pure translation of a model's JSON arguments
//! into the module's request vocabulary.
//!
//! A host registers one model-visible tool per [`DesktopTool`], taking the
//! name, description and schema from here. At call time it applies its own
//! approval and enablement policy, builds the request with the matching
//! function below, and sends it to the member the request names. Nothing here
//! performs I/O.

use serde_json::{json, Value};

use crate::{
    names, FindRequest, LaunchRequest, ListAppsRequest, ListWindowsRequest, RefRequest,
    RunGoalRequest, SnapshotRequest, TypeRequest, VisiblePredicate,
};

/// The tool family every desktop tool belongs to.
pub const FAMILY: &str = "desktop";

/// Default and bounds of a goal's wall-clock budget, in milliseconds.
pub const GOAL_DEFAULT_ELAPSED_MS: u64 = 120_000;
/// Smallest accepted goal budget, in milliseconds.
pub const GOAL_MIN_ELAPSED_MS: u64 = 1_000;
/// Largest accepted goal budget, in milliseconds.
pub const GOAL_MAX_ELAPSED_MS: u64 = 300_000;
/// Slack added to a goal's budget for the tool-call timeout, in milliseconds.
pub const GOAL_TIMEOUT_SLACK_MS: u64 = 30_000;
/// Tool-call timeout of a resumed goal, in milliseconds.
pub const CONTINUE_GOAL_TIMEOUT_MS: u64 = 330_000;

/// One desktop tool, in catalogue order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DesktopTool {
    /// List running applications.
    Apps,
    /// List an application's native windows.
    Windows,
    /// Launch or activate an application.
    Launch,
    /// Snapshot an accessibility tree.
    Snapshot,
    /// Find an element by role and name.
    Find,
    /// Act on an element ref.
    Act,
    /// Run one bounded goal.
    Goal,
    /// Resume a goal after the user approved its pending action.
    ContinueGoal,
}

impl DesktopTool {
    /// Every tool, in the order a host persists its catalogue.
    pub const ALL: [Self; 8] = [
        Self::Apps,
        Self::Windows,
        Self::Launch,
        Self::Snapshot,
        Self::Find,
        Self::Act,
        Self::Goal,
        Self::ContinueGoal,
    ];

    /// The model-visible tool name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Apps => "desktop_list_apps",
            Self::Windows => "desktop_list_windows",
            Self::Launch => "desktop_launch",
            Self::Snapshot => "desktop_snapshot",
            Self::Find => "desktop_find",
            Self::Act => "desktop_act",
            Self::Goal => "desktop_goal",
            Self::ContinueGoal => "desktop_continue_goal",
        }
    }

    /// The tool whose model-visible name is `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// The one-line description shown to the model.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Apps => "List running desktop applications on this computer.",
            Self::Windows => "List native windows for a running desktop app.",
            Self::Launch => {
                "Launch or activate a named desktop app so its window becomes available."
            }
            Self::Snapshot => {
                "Inspect an app's accessibility tree and obtain snapshot-qualified element refs."
            }
            Self::Find => "Find a desktop accessibility element by role and name, returning a ref.",
            Self::Act => {
                "Act on a desktop element ref: click, focus, type, check, uncheck, expand, or collapse."
            }
            Self::Goal => {
                "Run one scoped, bounded Jev desktop task through its internal observe-act-verify loop. Supply a visible success predicate; discover via tool_search."
            }
            Self::ContinueGoal => {
                "Resume a desktop goal after the user approved its exact pending action in Connections."
            }
        }
    }

    /// The recorded parameter schema as JSON text (draft-07 object schema).
    #[must_use]
    pub const fn parameters_json(self) -> &'static str {
        match self {
            Self::Apps => include_str!("parameters/desktop_apps.json"),
            Self::Windows => include_str!("parameters/desktop_windows.json"),
            Self::Launch => include_str!("parameters/desktop_launch.json"),
            Self::Snapshot => include_str!("parameters/desktop_snapshot.json"),
            Self::Find => include_str!("parameters/desktop_find.json"),
            Self::Act => include_str!("parameters/desktop_act.json"),
            Self::Goal => include_str!("parameters/desktop_goal.json"),
            Self::ContinueGoal => include_str!("parameters/desktop_continue_goal.json"),
        }
    }

    /// The recorded parameter schema, parsed. `Null` only if the recorded
    /// text were invalid, which a test pins against.
    #[must_use]
    pub fn parameters_schema(self) -> Value {
        serde_json::from_str(self.parameters_json()).unwrap_or(Value::Null)
    }

    /// Whether the tool changes the desktop (everything but the read-only
    /// listing, snapshot and find tools).
    #[must_use]
    pub const fn mutates(self) -> bool {
        matches!(
            self,
            Self::Act | Self::Goal | Self::ContinueGoal | Self::Launch
        )
    }

    /// Whether the tool needs the Accessibility permission (the listing and
    /// launch tools do not).
    #[must_use]
    pub const fn needs_accessibility(self) -> bool {
        !matches!(self, Self::Apps | Self::Windows | Self::Launch)
    }

    /// The tool-call timeout in milliseconds, or `None` to inherit the
    /// host's default.
    #[must_use]
    pub fn timeout_ms(self, args: &Value) -> Option<u64> {
        match self {
            Self::Goal => Some(goal_budget_ms(args) + GOAL_TIMEOUT_SLACK_MS),
            Self::ContinueGoal => Some(CONTINUE_GOAL_TIMEOUT_MS),
            _ => None,
        }
    }
}

/// Why tool arguments could not be turned into a request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ToolArgError {
    /// A required parameter is absent, blank, or not text.
    #[error("missing required parameter: {0}")]
    Missing(&'static str),
    /// The arguments are present but not acceptable.
    #[error("{0}")]
    Invalid(String),
    /// The `operation` of `desktop_act` is not one of the supported set.
    #[error("Unsupported desktop operation")]
    UnsupportedOperation,
}

/// The trimmed, non-blank text at `key`.
///
/// # Errors
///
/// [`ToolArgError::Missing`] when absent, blank or not a string.
pub fn required(args: &Value, key: &'static str) -> Result<String, ToolArgError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(ToolArgError::Missing(key))
}

/// The text at `key` when the key is present; present-but-blank is an error.
///
/// # Errors
///
/// [`ToolArgError::Missing`] when the key is present but blank or not text.
pub fn optional_string(args: &Value, key: &'static str) -> Result<Option<String>, ToolArgError> {
    args.get(key).map(|_| required(args, key)).transpose()
}

fn text(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// The goal's wall-clock budget requested by `args`, clamped.
#[must_use]
pub fn goal_budget_ms(args: &Value) -> u64 {
    args.get("max_elapsed_ms")
        .and_then(Value::as_u64)
        .unwrap_or(GOAL_DEFAULT_ELAPSED_MS)
        .clamp(GOAL_MIN_ELAPSED_MS, GOAL_MAX_ELAPSED_MS)
}

/// `desktop_list_apps`: the member and its request.
#[must_use]
pub fn apps_request() -> (&'static str, ListAppsRequest) {
    (names::methods::LIST_APPS, ListAppsRequest::default())
}

/// `desktop_list_windows`: the member and its request.
#[must_use]
pub fn windows_request(args: &Value) -> (&'static str, ListWindowsRequest) {
    (
        names::methods::LIST_WINDOWS,
        ListWindowsRequest {
            app: text(args, "app"),
        },
    )
}

/// `desktop_launch`: the member and its request (activating, attaching when
/// already running; never accepting process arguments or environment).
///
/// # Errors
///
/// [`ToolArgError::Missing`] when `app` is absent or blank.
pub fn launch_request(args: &Value) -> Result<(&'static str, LaunchRequest), ToolArgError> {
    let mut request = LaunchRequest::new(required(args, "app")?);
    request.activate = true;
    request.attach_if_running = Some(true);
    Ok((names::methods::LAUNCH, request))
}

/// `desktop_snapshot`: the member and its request.
///
/// # Errors
///
/// [`ToolArgError::Missing`] when `window_id` is present but blank.
pub fn snapshot_request(args: &Value) -> Result<(&'static str, SnapshotRequest), ToolArgError> {
    Ok((
        names::methods::SNAPSHOT,
        SnapshotRequest {
            app: text(args, "app"),
            window_id: optional_string(args, "window_id")?,
            skeleton: args
                .get("skeleton")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            root_ref: text(args, "root_ref"),
            max_depth: args
                .get("max_depth")
                .and_then(Value::as_u64)
                .map(|n| n.clamp(1, 12) as u8),
            ..SnapshotRequest::default()
        },
    ))
}

/// `desktop_find`: the member and its request (limit defaults to 10, max 20).
#[must_use]
pub fn find_request(args: &Value) -> (&'static str, FindRequest) {
    (
        names::methods::FIND,
        FindRequest {
            app: text(args, "app"),
            role: text(args, "role"),
            name: text(args, "name"),
            root: text(args, "root"),
            limit: Some(
                args.get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
                    .clamp(1, 20) as usize,
            ),
            ..FindRequest::default()
        },
    )
}

/// The request `desktop_act` resolves to.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ActRequest {
    /// `Type` with the text to enter.
    Type(TypeRequest),
    /// Any other operation, addressed by ref only.
    Ref(RefRequest),
}

/// `desktop_act`: the member and its request.
///
/// # Errors
///
/// [`ToolArgError::Missing`] for a blank `ref_id`, `operation` or (for
/// `type`) `text`; [`ToolArgError::UnsupportedOperation`] for an unknown
/// operation.
pub fn act_request(args: &Value) -> Result<(&'static str, ActRequest), ToolArgError> {
    let reference = required(args, "ref_id")?;
    let operation = required(args, "operation")?;
    if operation == "type" {
        let text = required(args, "text")?;
        return Ok((
            names::methods::TYPE,
            ActRequest::Type(TypeRequest {
                ref_id: reference,
                text,
                ..TypeRequest::default()
            }),
        ));
    }
    let member = match operation.as_str() {
        "click" => names::methods::CLICK,
        "focus" => names::methods::FOCUS,
        "check" => names::methods::CHECK,
        "uncheck" => names::methods::UNCHECK,
        "expand" => names::methods::EXPAND,
        "collapse" => names::methods::COLLAPSE,
        _ => return Err(ToolArgError::UnsupportedOperation),
    };
    Ok((member, ActRequest::Ref(RefRequest::new(reference))))
}

/// `desktop_goal`: the `RunGoal` request, validated.
///
/// `approvals_enabled` becomes `require_confirmations`. Malformed predicates
/// and operation names fail here, before anything reaches the module.
///
/// # Errors
///
/// [`ToolArgError`] when `app`, `goal`, `allowed_operations`,
/// `allowed_targets` or `success` is missing or unusable, a success
/// predicate has a blank field, or a `text_slots` key is not an allowed
/// target.
pub fn goal_request(
    args: &Value,
    approvals_enabled: bool,
) -> Result<(&'static str, RunGoalRequest), ToolArgError> {
    let app = required(args, "app")?;
    let goal = required(args, "goal")?;
    let invalid = |message: &str| ToolArgError::Invalid(message.to_owned());
    let operations = args
        .get("allowed_operations")
        .and_then(Value::as_array)
        .filter(|operations| !operations.is_empty())
        .ok_or_else(|| invalid("desktop_goal requires allowed_operations"))?;
    let targets = args
        .get("allowed_targets")
        .and_then(Value::as_array)
        .filter(|targets| {
            !targets.is_empty()
                && targets
                    .iter()
                    .all(|target| target.as_str().is_some_and(|name| !name.trim().is_empty()))
        })
        .ok_or_else(|| {
            invalid(
                "needs_inspection: supply allowed_targets using exact accessible names, descriptions, or native AX identifiers from desktop_snapshot or desktop_find",
            )
        })?;
    let success = args
        .get("success")
        .and_then(Value::as_array)
        .filter(|predicates| !predicates.is_empty())
        .ok_or_else(|| invalid("desktop_goal requires a visible success predicate"))?;
    let mut request = json!({
        "app": app,
        "goal": goal,
        "include_values": false,
        "require_confirmations": approvals_enabled,
        "max_steps": args.get("max_steps").and_then(Value::as_u64).unwrap_or(12).clamp(1, 20),
        "max_model_calls": args.get("max_model_calls").and_then(Value::as_u64).unwrap_or(24).clamp(1, 40),
        "max_elapsed_ms": goal_budget_ms(args),
        "success": success,
        "text_slots": args.get("text_slots").cloned().unwrap_or_else(|| json!({})),
        "allowed_operations": operations,
        "allowed_targets": targets,
    });
    if let Some(window) = args.get("window") {
        request["window"] = window.clone();
    }
    if let Some(window_id) = optional_string(args, "window_id")? {
        request["window_id"] = json!(window_id);
    }
    let request: RunGoalRequest = serde_json::from_value(request)
        .map_err(|error| ToolArgError::Invalid(error.to_string()))?;
    if !request.success.iter().all(predicate_is_complete) {
        return Err(invalid(
            "desktop_goal success predicates require nonblank names, state tokens, fragments, and ancestor scopes",
        ));
    }
    if request
        .text_slots
        .keys()
        .any(|name| !request.allowed_targets.iter().any(|target| target == name))
    {
        return Err(invalid(
            "desktop_goal text_slots keys must match an exact allowed target",
        ));
    }
    Ok((names::methods::RUN_GOAL, request))
}

fn predicate_is_complete(predicate: &VisiblePredicate) -> bool {
    match predicate {
        VisiblePredicate::NamePresent { name } | VisiblePredicate::ValueEquals { name, .. } => {
            !name.trim().is_empty()
        }
        VisiblePredicate::NameContains { fragment, within } => {
            !fragment.trim().is_empty() && !within.trim().is_empty()
        }
        VisiblePredicate::ValueContains { name, value } => {
            !name.trim().is_empty() && !value.trim().is_empty()
        }
        VisiblePredicate::StateContains { name, state } => {
            !name.trim().is_empty() && !state.trim().is_empty()
        }
    }
}

#[cfg(test)]
mod desktop_tool_tests;
