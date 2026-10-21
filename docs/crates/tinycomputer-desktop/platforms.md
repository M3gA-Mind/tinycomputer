# Platforms

`tinycomputer-desktop` picks its accessibility backend at compile time,
based on the target operating system:

```rust
#[cfg(target_os = "macos")]
{ agent_desktop_macos::MacOSAdapter::new() }

#[cfg(target_os = "windows")]
{ agent_desktop_windows::WindowsAdapter::new() }

#[cfg(target_os = "linux")]
{ agent_desktop_linux::LinuxAdapter::new() }
```

Building for anything else is a compile error, not a runtime one: the crate
does not try to guess a fallback for an unknown platform. See
`platform_adapter` in
[`crates/tinycomputer-desktop/src/desktop/mod.rs`](../../../crates/tinycomputer-desktop/src/desktop/mod.rs).

## What that means today, per platform

| Platform | Status |
|---|---|
| macOS | Full accessibility backend. Every member works, subject to the permissions covered in [Permissions](permissions.md). |
| Windows | Not yet supported. Builds, loads, and answers every call, but the backend implements no accessibility surfaces yet. Every observation there fails with `PLATFORM_NOT_SUPPORTED`, and the error lists which surfaces the backend does support: none, currently. |
| Linux | Not yet supported, same as Windows. |

This is not a limitation this crate adds; it is inherited directly from the
vendored `agent-desktop` engine, and it will change as soon as the engine's
Windows and Linux backends gain surfaces, with no change needed here. If you
are trying to run tinycomputer's desktop automation on Windows or Linux today,
expect `PLATFORM_NOT_SUPPORTED` on anything that touches an application's
tree.

## Why this crate does not try to work around it

The instruction that governs this whole crate applies here too: this
repository is an adapter, not an engine. A wrong tree, a missing platform
surface, or an unsupported operation on one platform is a bug (or, on
Windows and Linux, a known gap) in `vendor/agent-desktop`, fixed there and picked up in
this repository as a pinned dependency bump. Patching around it here would
just mean the fix has to be redone the moment the vendored dependency
catches up.

## Where permission mechanics differ by platform

Both macOS and Windows gate accessibility and screen recording behind
operating-system permission prompts, described in
[Permissions](permissions.md). The exact settings a user has to grant
differ (System Settings on macOS, an equivalent on Windows), but the code in
this crate treats them uniformly through the same `PermissionReport` and
the same `PERM_DENIED` error code; it does not branch on platform to decide
whether a permission check applies.

## Related reading

- [Permissions](permissions.md) for what each member needs and how a denied
  permission is reported.
- [Errors and the envelope](errors-and-the-envelope.md) for the shape of
  `PLATFORM_NOT_SUPPORTED` and every other error code.
- `crates/tinycomputer-desktop/README.md`'s crate table and
  `docs/technical/architecture.md` for where the platform backends fit
  relative to the rest of tinycomputer.
