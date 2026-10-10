//! Unit tests for the focus module's bounded command runner.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(unix)]
use super::*;

#[cfg(unix)]
#[test]
fn command_output_with_timeout_returns_output_for_fast_command() {
    let mut command = Command::new("sh");
    command.arg("-c").arg("printf ready");

    let output =
        command_output_with_timeout("test fast command", &mut command, Duration::from_secs(1))
            .expect("fast command should complete");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ready");
}

#[cfg(unix)]
#[test]
fn command_output_with_timeout_drains_large_output_while_waiting() {
    let mut command = Command::new("sh");
    command.arg("-c").arg("head -c 200000 /dev/zero");

    let output = command_output_with_timeout(
        "test large output command",
        &mut command,
        Duration::from_secs(2),
    )
    .expect("large output should not fill the pipe and block the child");

    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 200_000);
}

#[cfg(unix)]
#[test]
fn command_output_with_timeout_kills_slow_command() {
    let mut command = Command::new("sh");
    command.arg("-c").arg("sleep 2; printf late");

    let error =
        command_output_with_timeout("test slow command", &mut command, Duration::from_millis(50))
            .expect_err("slow command should time out");

    assert!(error.contains("timed out after"));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn public_focus_query_returns_typed_unsupported_error() {
    let error = super::focused_text_context().expect_err("focus querying is macOS-only");
    assert!(matches!(error, super::Error::UnsupportedPlatform));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn unsupported_platform_rejects_unverifiable_focus_targets() {
    assert!(super::validate_focused_target(None, None, None).is_ok());
    assert!(matches!(
        super::validate_focused_target(Some("Editor"), None, None),
        Err(super::Error::UnsupportedPlatform)
    ));
}
