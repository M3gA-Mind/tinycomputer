//! Swift target selection for the embedded Globe listener.

/// Return Swift's target triple for the Rust macOS target architecture.
pub(crate) fn triple(
    architecture: &str,
    deployment_target: Option<&str>,
) -> Result<String, String> {
    let (swift_arch, default_deployment) = match architecture {
        "aarch64" => ("arm64", "11.0"),
        "x86_64" => ("x86_64", "10.12"),
        other => return Err(format!("unsupported macOS target architecture: {other}")),
    };
    let deployment = deployment_target.unwrap_or(default_deployment);
    if deployment.is_empty()
        || !deployment
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || !deployment.contains('.')
    {
        return Err("invalid macOS deployment target".to_string());
    }
    Ok(format!("{swift_arch}-apple-macosx{deployment}"))
}

#[cfg(test)]
#[path = "swift_target_tests.rs"]
mod tests;
