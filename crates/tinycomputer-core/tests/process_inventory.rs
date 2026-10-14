//! Every process spawn in this workspace is listed in
//! `docs/technical/specs/process-containment.md`, so a new one cannot slip in
//! unsandboxable without a documented decision.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_spawning_file_is_documented() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc = fs::read_to_string(root.join("docs/technical/specs/process-containment.md"))
        .expect("process-containment.md exists");
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let mut undocumented = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.contains("/tests/")
            || rel.contains("_tests.rs")
            || rel.contains("tinycomputer-examples")
        {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let spawns = text.contains("Command::new(");
        if !spawns {
            continue;
        }
        // The doc names files as `crates/<crate>/src/<path>`, sometimes with
        // `{a,b}` grouping, so match on the file name within its crate.
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        let krate = rel.split('/').nth(1).unwrap();
        let listed = doc.lines().any(|l| l.contains(krate) && l.contains(&name));
        if !listed {
            undocumented.push(rel);
        }
    }
    assert!(
        undocumented.is_empty(),
        "spawn sites missing from process-containment.md: {undocumented:?}"
    );
}
