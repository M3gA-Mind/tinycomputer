//! The module's private `planner` configuration: which models plan, rescue,
//! and shape a task, and the route each is called through.
//!
//! A route is a provider ([`LanguageModelProvider`]), its credential, and at
//! most an exact repeat of that provider's one approved base URL — the same
//! allow-list principle as Jev's endpoint, so private configuration can never
//! point a model's key at an arbitrary host.

use serde::Deserialize;
use tinycomputer_bus::agent::{LanguageModelConfiguration, LanguageModelProvider};

/// The model used when the configuration names none.
pub const PLANNER_MODEL: &str = "anthropic/claude-sonnet-5";

/// The reasoning model a failed step is rescued with when the configuration
/// names none.
pub const RESCUE_MODEL: &str = "openai/gpt-6-luna";

/// The reasoning model a finished task's answer is shaped with when the
/// configuration names none.
pub const OUTPUT_MODEL: &str = "openai/gpt-6-luna";

/// `OpenRouter`'s OpenAI-compatible base URL, the only one the
/// `open_router` route accepts.
pub const OPEN_ROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Tiny Humans' OpenAI-compatible gateway, the only base URL the
/// `tiny_humans` route accepts.
pub const TINYHUMANS_BASE_URL: &str = "https://api.tinyhumans.ai/openai/v1";

/// The module's private `planner` configuration.
///
/// Its route fields (`api_key`, `provider`, `endpoint_url`, `sdk_name`) sit
/// at the top level of the object, beside the three models. The planner and
/// the shaper always take that route; the rescuer takes `rescue_route` when
/// one is given.
#[derive(Clone, Deserialize)]
pub struct PlannerConfig {
    /// The route the planner, the shaper, and (without `rescue_route`) the
    /// rescuer call their models through.
    #[serde(flatten)]
    pub route: ModelRoute,
    /// The model id; [`PLANNER_MODEL`] when absent.
    #[serde(default)]
    pub model: Option<String>,
    /// The model id failed steps are rescued with; [`RESCUE_MODEL`] when
    /// absent.
    #[serde(default)]
    pub rescue_model: Option<String>,
    /// The model id a finished task's answer is shaped with;
    /// [`OUTPUT_MODEL`] when absent.
    #[serde(default)]
    pub output_model: Option<String>,
    /// A separate route, with its own key, for the rescuer alone. Nothing is
    /// inherited from the planner's route: it is a complete route of its own.
    #[serde(default)]
    pub rescue_route: Option<ModelRoute>,
}

impl PlannerConfig {
    /// The route the rescuer calls through: `rescue_route`, or the planner's.
    #[must_use]
    pub fn rescuer_route(&self) -> &ModelRoute {
        self.rescue_route.as_ref().unwrap_or(&self.route)
    }
}

impl std::fmt::Debug for PlannerConfig {
    /// Never prints a key.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlannerConfig")
            .field("route", &self.route)
            .field("model", &self.model)
            .field("rescue_model", &self.rescue_model)
            .field("output_model", &self.output_model)
            .field("rescue_route", &self.rescue_route)
            .finish()
    }
}

/// Where a language model's requests go, and with which credential.
#[derive(Clone, Default, Deserialize)]
pub struct ModelRoute {
    /// The provider's credential: an `OpenRouter` key, or the host's
    /// `TinyHumans` bearer.
    pub api_key: String,
    /// The OpenAI-compatible route; `open_router` when absent.
    #[serde(default)]
    pub provider: LanguageModelProvider,
    /// The provider's approved base URL, repeated exactly; any other value is
    /// refused.
    #[serde(default)]
    pub endpoint_url: Option<String>,
    /// Host product attribution, sent as `x-sdk-name` to the `tiny_humans`
    /// route only.
    #[serde(default)]
    pub sdk_name: Option<String>,
}

/// A host-owned OpenAI-compatible model endpoint.
///
/// Use one configuration per role when planner, rescue, or output models need
/// different routes. The key is neither serializable nor printed by `Debug`.
#[derive(Clone, Deserialize)]
pub struct HostModelConfig {
    /// The host's provider credential.
    pub api_key: String,
    /// The provider's OpenAI-compatible base URL.
    pub base_url: String,
    /// Model id understood by that endpoint.
    pub model: String,
}

impl std::fmt::Debug for HostModelConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostModelConfig")
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

/// Independent host-supplied model configurations for each task role.
#[derive(Debug, Clone, Deserialize)]
pub struct HostModels {
    /// Model used to draft plain-language tasks.
    pub planner: HostModelConfig,
    /// Model used to recover failed steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rescue: Option<HostModelConfig>,
    /// Model used to shape a completed task's output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<HostModelConfig>,
}

impl std::fmt::Debug for ModelRoute {
    /// Never prints the key.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelRoute")
            .field("api_key", &"[REDACTED]")
            .field("provider", &self.provider)
            .field("endpoint_url", &self.endpoint_url)
            .field("sdk_name", &self.sdk_name)
            .finish()
    }
}

/// A route checked and ready to build a model on.
pub(super) struct ResolvedRoute {
    pub(super) provider: LanguageModelProvider,
    pub(super) base_url: String,
    pub(super) sdk_name: Option<String>,
}

impl ModelRoute {
    /// This route checked: a key present, and an endpoint, if any, that is
    /// the provider's own.
    pub(super) fn resolve(&self) -> Result<ResolvedRoute, String> {
        if self.api_key.trim().is_empty() {
            return Err("the planner configuration needs an api_key".to_owned());
        }
        if self.provider == LanguageModelProvider::OpenAiCompatible {
            return Err(
                "open_ai_compatible routes are available only to Rust embedding hosts".to_owned(),
            );
        }
        let base_url = match &self.endpoint_url {
            Some(endpoint) if trusted_base_url(self.provider, endpoint) => {
                endpoint.trim_end_matches('/').to_owned()
            }
            Some(_) => {
                return Err("endpoint_url is not an approved language model route".to_owned());
            }
            None => approved_base_url(self.provider).to_owned(),
        };
        let sdk_name = match self.provider {
            LanguageModelProvider::TinyHumans => {
                self.sdk_name.as_deref().and_then(sanitized_sdk_name)
            }
            LanguageModelProvider::OpenRouter | LanguageModelProvider::OpenAiCompatible => None,
        };
        Ok(ResolvedRoute {
            provider: self.provider,
            base_url,
            sdk_name,
        })
    }

    /// The non-secret summary of `model` on this route.
    pub(super) fn describe(&self, model: &str) -> LanguageModelConfiguration {
        LanguageModelConfiguration {
            provider: self.provider,
            model: model.to_owned(),
            endpoint_url: self.endpoint_url.clone(),
        }
    }
}

/// The one base URL each route may be configured at.
#[must_use]
pub(super) const fn approved_base_url(provider: LanguageModelProvider) -> &'static str {
    match provider {
        LanguageModelProvider::OpenRouter => OPEN_ROUTER_BASE_URL,
        LanguageModelProvider::TinyHumans => TINYHUMANS_BASE_URL,
        LanguageModelProvider::OpenAiCompatible => "",
    }
}

/// Whether `endpoint` is `provider`'s approved base URL, with or without a
/// trailing slash.
pub(super) fn trusted_base_url(provider: LanguageModelProvider, endpoint: &str) -> bool {
    if matches!(provider, LanguageModelProvider::OpenAiCompatible) {
        return false;
    }
    if endpoint.trim_end_matches('/') == approved_base_url(provider) {
        return true;
    }
    #[cfg(test)]
    return endpoint.starts_with("http://127.0.0.1:");
    #[cfg(not(test))]
    false
}

/// `raw` as a header-safe product name, as the Jev client sanitizes its own:
/// ASCII alphanumerics, `.`, `_`, and `-`, lowercased, at most 64 bytes.
fn sanitized_sdk_name(raw: &str) -> Option<String> {
    let name = raw
        .trim()
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        .take(64)
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();
    (!name.is_empty()).then_some(name)
}
