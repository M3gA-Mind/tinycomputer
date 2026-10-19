//! Starting a session: the explicit launch and the viewport.
//!
//! The allowed origins are not handed to the engine: agent-browser would
//! refuse every request outside them, a page's own files and APIs included,
//! and refuse a profile beside them. The session checks pages itself
//! (`crate::origins`).

use serde_json::{Value, json};
use tinycomputer_bus::browser::{SessionOptions, UserAgentBrand, UserAgentMetadata};

/// The explicit `launch` every session starts with.
///
/// Sending it explicitly matters: without one, agent-browser auto-launches
/// from the `AGENT_BROWSER_*` environment of whatever process hosts the
/// module, which is not this session's configuration.
#[must_use]
pub(crate) fn launch(options: &SessionOptions) -> Value {
    let mut command = json!({
        "action": "launch",
        "headless": options.headless,
        "args": options.args,
    });
    let fields = [
        ("cdpUrl", options.endpoint.as_ref()),
        ("executablePath", options.executable.as_ref()),
        ("userAgent", options.user_agent.as_ref()),
        ("profile", options.user_data_dir.as_ref()),
        ("downloadPath", options.download_dir.as_ref()),
        ("timezone", options.timezone.as_ref()),
        ("locale", options.locale.as_ref()),
        ("acceptLanguage", options.accept_language.as_ref()),
    ];
    for (key, value) in fields {
        if let Some(value) = value {
            command[key] = json!(value);
        }
    }
    if let Some(metadata) = &options.user_agent_metadata {
        command["userAgentMetadata"] = user_agent_metadata(metadata);
    }
    if let Some(screen) = &options.screen {
        command["screen"] = json!({
            "width": screen.width,
            "height": screen.height,
            "deviceScaleFactor": screen.device_scale_factor,
        });
    }
    if !options.init_scripts.is_empty() {
        command["initScriptSources"] = json!(options.init_scripts);
    }
    if options.stealth {
        command["stealth"] = json!(true);
    }
    command
}

/// The client hints in the engine's field names; fields left unset are left
/// out, as the engine reads an absent list differently from an empty one.
fn user_agent_metadata(metadata: &UserAgentMetadata) -> Value {
    let brands = |list: &[UserAgentBrand]| -> Value {
        list.iter()
            .map(|entry| json!({"brand": entry.brand, "version": entry.version}))
            .collect()
    };
    let mut value = json!({
        "platform": metadata.platform,
        "platformVersion": metadata.platform_version,
        "architecture": metadata.architecture,
        "model": metadata.model,
        "mobile": metadata.mobile,
    });
    if let Some(list) = &metadata.brands {
        value["brands"] = brands(list);
    }
    if let Some(list) = &metadata.full_version_list {
        value["fullVersionList"] = brands(list);
    }
    if let Some(version) = &metadata.full_version {
        value["fullVersion"] = json!(version);
    }
    if let Some(bitness) = &metadata.bitness {
        value["bitness"] = json!(bitness);
    }
    if let Some(wow64) = metadata.wow64 {
        value["wow64"] = json!(wow64);
    }
    value
}

/// The viewport a session applies straight after launching.
///
/// It replaces the launch's pixel ratio, so a session with a screen takes the
/// screen's: what `devicePixelRatio` reports is part of that screen.
#[must_use]
pub(crate) fn viewport(options: &SessionOptions) -> Value {
    let scale = options
        .screen
        .map_or(options.viewport.device_scale_factor, |screen| {
            screen.device_scale_factor
        });
    json!({
        "action": "viewport",
        "width": options.viewport.width,
        "height": options.viewport.height,
        "deviceScaleFactor": scale,
        "mobile": options.viewport.mobile,
    })
}
