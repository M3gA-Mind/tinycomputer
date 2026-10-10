//! Atomic installation for a build-time embedded native helper.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn install(cache_dir: &Path, binary_path: &Path, bytes: &[u8]) -> Result<bool, String> {
    if binary_path.parent() != Some(cache_dir) {
        return Err("embedded Globe listener helper is outside its cache directory".to_owned());
    }
    if existing_helper_matches(binary_path, bytes) {
        return Ok(false);
    }

    let (temporary_path, mut file) = create_temporary(cache_dir)?;
    let result: Result<(), String> = (|| {
        file.write_all(bytes)
            .map_err(|_| "failed to write embedded Globe listener helper".to_owned())?;
        file.sync_all()
            .map_err(|_| "failed to flush embedded Globe listener helper".to_owned())?;
        drop(file);
        // The temporary file is in the same private directory, so rename
        // publishes complete bytes in one step and replaces a destination
        // symlink itself instead of following it.
        fs::rename(&temporary_path, binary_path)
            .map_err(|_| "failed to install embedded Globe listener helper".to_owned())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result?;
    Ok(true)
}

fn existing_helper_matches(binary_path: &Path, bytes: &[u8]) -> bool {
    let Ok(metadata) = fs::symlink_metadata(binary_path) else {
        return false;
    };
    if !metadata.file_type().is_file() || !permissions_are_private(&metadata) {
        return false;
    }
    fs::read(binary_path).is_ok_and(|existing| existing == bytes)
}

fn permissions_are_private(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777 == 0o700
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        true
    }
}

fn create_temporary(parent: &Path) -> Result<(std::path::PathBuf, std::fs::File), String> {
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".globe-listener-{}-{sequence}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                return Err("failed to create embedded Globe listener temporary file".to_owned());
            }
        }
    }
    Err("failed to reserve embedded Globe listener temporary file".to_owned())
}

#[cfg(test)]
#[path = "helper_install_tests.rs"]
mod tests;
