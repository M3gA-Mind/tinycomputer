//! Successful browser actions observed by a trusted host.

/// The kind of a successful browser action, without its input payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedAction {
    /// A page navigation completed.
    Navigate,
    /// A control was clicked.
    Click,
    /// Escape was pressed.
    PressEscape,
    /// Another typed browser action completed.
    Other,
}

/// A semantic target resolved from the latest accessibility snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedTarget {
    /// The target's accessibility role.
    pub role: String,
    /// The target's accessible name, if present.
    pub name: Option<String>,
    /// The snapshot reference actually dispatched.
    pub reference: Option<String>,
    /// True only when the current snapshot contains exactly one target with
    /// this role and non-empty name, and the dispatched reference names it.
    pub unique: bool,
}

/// Metadata for a successful dispatch. It deliberately carries no request
/// payload, typed text, fill values, or locator source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionObservation {
    /// The action that completed.
    pub action: ObservedAction,
    /// The page immediately before dispatch.
    pub before_url: String,
    /// The page after dispatch and origin checks.
    pub after_url: String,
    /// The resolved target, when the action named one.
    pub target: Option<ObservedTarget>,
}

/// A trusted host sink for successful browser action metadata.
pub trait ActionObserver: Send + Sync {
    /// Receives one successful dispatch. Implementations should return
    /// quickly and must not retain data they do not need.
    fn observed(&self, action: ActionObservation);
}
