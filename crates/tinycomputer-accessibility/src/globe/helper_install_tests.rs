//! Local fixtures verify private, atomic installation of the embedded helper.
#![allow(
    clippy::expect_used,
    reason = "filesystem fixtures use descriptive setup and assertion failures"
)]

use super::install;

#[cfg(unix)]
#[test]
fn replacing_a_cached_helper_symlink_does_not_modify_its_target() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().expect("temporary cache");
    let target = dir.path().join("outside-target");
    let destination = dir.path().join("globe-listener");
    std::fs::write(&target, b"sentinel").expect("sentinel target");
    symlink(&target, &destination).expect("symlink fixture");

    assert!(install(dir.path(), &destination, b"embedded helper").is_ok());
    assert_eq!(std::fs::read(&target).expect("read target"), b"sentinel");
    assert!(
        std::fs::symlink_metadata(&destination)
            .expect("installed helper metadata")
            .file_type()
            .is_file()
    );
    assert_eq!(
        std::fs::read(&destination).expect("installed helper"),
        b"embedded helper"
    );
    assert!(!install(dir.path(), &destination, b"embedded helper").expect("idempotent install"));
}

#[cfg(unix)]
#[test]
fn matching_helper_with_public_permissions_is_replaced_securely() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("temporary cache");
    let destination = dir.path().join("globe-listener");
    std::fs::write(&destination, b"embedded helper").expect("existing helper");
    std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o755))
        .expect("make fixture public");

    assert!(install(dir.path(), &destination, b"embedded helper").expect("secure replacement"));
    let metadata = std::fs::symlink_metadata(&destination).expect("installed metadata");
    assert!(metadata.file_type().is_file());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
}

#[cfg(unix)]
#[test]
fn an_install_failure_removes_its_private_temporary_file() {
    let dir = tempfile::tempdir().expect("temporary cache");
    let destination = dir.path().join("existing-directory");
    std::fs::create_dir(&destination).expect("destination directory");

    assert!(install(dir.path(), &destination, b"embedded helper").is_err());
    let entries = std::fs::read_dir(dir.path())
        .expect("cache directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("cache entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_name(), "existing-directory");
}

#[cfg(unix)]
#[test]
fn concurrent_helper_replacements_never_expose_partial_bytes() {
    let dir = tempfile::tempdir().expect("temporary cache");
    let destination = dir.path().join("globe-listener");
    let first = vec![b'a'; 2 * 1024 * 1024];
    let second = vec![b'b'; first.len()];
    install(dir.path(), &destination, &first).expect("initial install");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader_path = destination.clone();
    let reader_barrier = barrier.clone();
    let reader_finished = finished.clone();
    let reader_first = first.clone();
    let reader_second = second.clone();
    let reader = std::thread::spawn(move || {
        reader_barrier.wait();
        let mut observations = 0;
        while !reader_finished.load(std::sync::atomic::Ordering::Acquire) || observations < 128 {
            let bytes = std::fs::read(&reader_path).expect("published helper remains present");
            assert!(
                bytes == reader_first || bytes == reader_second,
                "reader observed a partial or mixed helper"
            );
            observations += 1;
        }
        observations
    });
    let workers = (0..8)
        .map(|index| {
            let path = destination.clone();
            let bytes = if index % 2 == 0 {
                first.clone()
            } else {
                second.clone()
            };
            let cache = dir.path().to_path_buf();
            let worker_barrier = barrier.clone();
            std::thread::spawn(move || {
                worker_barrier.wait();
                install(&cache, &path, &bytes)
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().expect("installer thread").expect("install");
    }
    finished.store(true, std::sync::atomic::Ordering::Release);
    assert!(reader.join().expect("reader thread") >= 128);
}
