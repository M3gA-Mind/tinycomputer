# TinyBus Module Release

## Purpose

Generated projects must be usable as native TinyBus integrations and
distributable without also shipping the TinyBus host runtime.

## Contract

- The library builds as both an `rlib` and a native `cdylib`.
- The `cdylib` exports TinyBus module ABI v1, an embedded manifest, and the
  initialization entrypoint.
- The module provides the fifty-nine members of
  `ai.tinyhumans.tinycomputer.Desktop` at `/ai/tinyhumans/tinycomputer/Desktop`.
- Each release archive is named
  `tinycomputer-<version>-<platform>.<extension>` and contains only this
  module, its SHA-256 `modules.toml`, license, and installation documentation
  — plus, on macOS, the engine's `agent-desktop-macos-helper`, which the
  engine's clipboard only trusts beside the loaded module.
- Each GitHub release publishes a separate `checksum.toml` mapping every
  archive filename to its SHA-256 digest for TinyBus's release loader.
- Release builds cover the stable native Ubuntu, macOS, and Windows runners,
  Fedora 43/44 containers, and rolling Arch Linux where official runners or
  images exist for the architecture.
- TinyBus itself remains a pinned SDK submodule and is not shipped as a release
  asset from this repository.

## Verification

CI exercises the bus interface through TinyBus's in-memory transport, enforces
90% line coverage in every source file, and builds the `cdylib`. The release
workflow opens a checked version pull request before tagging and builds each
native module from the tagged source, recording its exact
digest in the adjacent allowlist. After publishing, it downloads the Ubuntu
x86_64 archive through TinyBus's GitHub release API and calls `Version` over an
in-memory bus. `Version` is chosen because it needs no granted permission and
touches no other application, so the gate tests the artifact rather than the
runner's configuration. Before loading the archived module, the workflow
installs its Linux runtime libraries and checks that `ldd` resolves every
dependency, so a missing host package cannot masquerade as a broken module.
