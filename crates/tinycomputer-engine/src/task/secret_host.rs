//! Host-resolved secret facts. Secret material is typed by the embedding
//! host and can be redacted from observations without returning it to engine.

/// Metadata needed to resolve and type one host-side secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretTypeRequest {
    /// Opaque host identifier. Never pass this to a model or report.
    pub handle: String,
    /// Safe mask associated with the secret fact.
    pub mask: String,
    /// Host-defined secret category.
    pub kind: String,
    /// Accessibility role of the selected field.
    pub field_role: String,
    /// Accessibility name of the selected field, when present.
    pub field_name: Option<String>,
    /// Snapshot reference identifying the selected field.
    pub field_ref: String,
    /// Current page URL. Secret typing is refused when this is unknown.
    pub current_url: String,
    /// Task whose browser session owns the field.
    pub task_id: String,
}

/// A host that resolves opaque handles and types secrets without returning
/// their values to the engine.
pub trait SecretHost: Send + Sync + 'static {
    /// Resolve and type into the requested field. A refusal must type nothing.
    fn type_secret(&self, request: &SecretTypeRequest) -> Result<(), String>;

    /// Replace any host-held secret values appearing in outbound text.
    fn redact(&self, task_id: &str, text: &str) -> String;
}
