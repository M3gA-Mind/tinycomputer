//! Installs the build-time embedded Swift listener into a private cache.
use super::LOG_PREFIX;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

const HELPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tinycomputer-globe-listener"));

pub(super) fn ensure_globe_helper_binary(
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<PathBuf, String> {
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("native_start_cancelled".into());
    }
    if HELPER.is_empty() {
        return Err("macOS Globe helper is unavailable in this cross-target build".into());
    }
    let cache_dir = super::super::helper::private_cache_dir("openhuman-globe-listener")?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    HELPER.hash(&mut hasher);
    let binary_path = cache_dir.join(format!("globe_listener_{:016x}", hasher.finish()));

    let installed = fs::read(&binary_path).is_ok_and(|bytes| bytes == HELPER);
    if !installed {
        fs::write(&binary_path, HELPER)
            .map_err(|_| "failed to install embedded Globe listener helper".to_owned())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary_path, fs::Permissions::from_mode(0o700))
                .map_err(|_| "failed to secure embedded Globe listener helper".to_owned())?;
        }
        log::debug!(
            "{LOG_PREFIX} installed embedded helper {}",
            binary_path.display()
        );
    }
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("native_start_cancelled".into());
    }
    Ok(binary_path)
}
