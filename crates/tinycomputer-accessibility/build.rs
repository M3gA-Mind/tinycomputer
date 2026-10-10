//! Compile the bundled macOS Globe listener for native macOS builds.
use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let source = PathBuf::from("src/globe/listener.swift");
    println!("cargo:rerun-if-changed={}", source.display());

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not set OUT_DIR")?);
    let binary = out_dir.join("tinycomputer-globe-listener");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    if target_os == "macos" && host.contains("apple-darwin") {
        let status = Command::new("swiftc")
            .args(["-O", "-framework", "Cocoa"])
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .status()
            .map_err(|error| {
                format!("could not run swiftc to embed the Globe listener: {error}")
            })?;
        if !status.success() {
            return Err("swiftc failed to build the Globe listener".into());
        }
    } else {
        // Cross-target checks can compile the macOS Rust path without a macOS
        // SDK. Such artifacts fail at listener startup instead of invoking a
        // compiler on the end user's machine.
        std::fs::write(&binary, [])?;
        if target_os == "macos" {
            println!(
                "cargo:warning=macOS Globe helper is unavailable when cross-compiling from {host}"
            );
        }
    }
    Ok(())
}
