//! Pure target-selection fixtures for the build-time Swift listener helper.
use super::triple;

#[test]
fn swift_target_matches_each_supported_rust_architecture() {
    assert_eq!(
        triple("aarch64", None).as_deref(),
        Ok("arm64-apple-macosx11.0")
    );
    assert_eq!(
        triple("x86_64", None).as_deref(),
        Ok("x86_64-apple-macosx10.12")
    );
}

#[test]
fn swift_target_uses_explicit_rust_deployment_target() {
    assert_eq!(
        triple("aarch64", Some("13.3")).as_deref(),
        Ok("arm64-apple-macosx13.3")
    );
}

#[test]
fn swift_target_refuses_unknown_architecture_and_invalid_deployment_target() {
    assert!(triple("riscv64", None).is_err());
    assert!(triple("aarch64", Some("11;malicious-flag")).is_err());
}
