//! Host-resolved secret facts. Secret material is typed by the embedding
//! host and can be redacted from observations without returning it to engine.

/// Metadata needed to resolve and type one host-side secret.
#[derive(Clone, PartialEq, Eq)]
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
    /// Task actions available before this host typing operation is charged.
    pub actions_remaining: u32,
}

impl std::fmt::Debug for SecretTypeRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretTypeRequest")
            .field("handle", &"[opaque]")
            .field("mask", &self.mask)
            .field("kind", &self.kind)
            .field("field_role", &self.field_role)
            .field("field_name", &self.field_name)
            .field("field_ref", &self.field_ref)
            .field("current_url", &self.current_url)
            .field("task_id", &self.task_id)
            .field("actions_remaining", &self.actions_remaining)
            .finish()
    }
}

/// A host that resolves opaque handles and types secrets without returning
/// their values to the engine.
pub trait SecretHost: Send + Sync + 'static {
    /// Resolve and type into the requested field. A refusal must type nothing.
    ///
    /// # Errors
    ///
    /// Returns a safe reason when the handle, task, page, field, site, or
    /// host policy refuses the type.
    fn type_secret(&self, request: &SecretTypeRequest) -> Result<(), String>;

    /// Replace any host-held secret values appearing in outbound text,
    /// including their textual representations as JSON numbers and keys.
    fn redact(&self, task_id: &str, text: &str) -> String;
}

#[cfg(test)]
mod tests {
    use super::SecretTypeRequest;

    #[test]
    fn debug_hides_the_opaque_handle_and_keeps_safe_type_context() {
        let request = SecretTypeRequest {
            handle: "secret-value-must-not-appear".into(),
            mask: "•••• 1111".into(),
            kind: "payment_card".into(),
            field_role: "textbox".into(),
            field_name: Some("Card number".into()),
            field_ref: "field-7".into(),
            current_url: "https://shop.example/checkout".into(),
            task_id: "task-2".into(),
            actions_remaining: 3,
        };

        let debug = format!("{request:?}");

        assert!(!debug.contains(&request.handle));
        assert!(debug.contains("[opaque]"));
        assert!(debug.contains("field-7"));
        assert!(debug.contains("actions_remaining: 3"));
    }
}
