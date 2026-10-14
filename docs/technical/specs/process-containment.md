# Process containment and egress

Status: Accepted. Owner: tinycomputer maintainers.
Part of tinyhumansai/openhuman#7328 (phase 5, sandbox coverage).

## Problem

A host that sandboxes spawned processes (OpenHuman's tinysecurity and tinybox
work) needs to know which processes this module starts, what network it can
reach, and which of those the host can contain. A process that is silently
skipped looks contained when it is not.

## Decision

tinycomputer is a native module loaded into the host process. It drives the
user's real desktop and a real Chrome, so it cannot run inside a jail that
hides those. This module therefore **does not expose a sandbox, proxy or egress
contract**: none of it is enforceable from inside the module, and a contract
that is not enforced is worse than none. The host treats the whole module as
unsandboxed-by-design and gates it at the member level (approval, payment stop,
`POLICY_DENIED`). This document is the explicit list of what is not contained.

## Process inventory

`crates/tinycomputer-core/tests/process_inventory.rs` fails if a crate under
`crates/` spawns a process from a file that is not named here.

| Process | Spawned from | Contract |
| --- | --- | --- |
| Chrome (browser sessions) | agent-browser (`vendor/agent-browser`, `native/cdp/chrome.rs`), started by an explicit `launch` | Unsupported. Runs as the user with their network. agent-browser can take a `proxy` launch option, but this module does not set or forward one, so egress is not enforceable here. Xvfb and certutil helpers are agent-browser's. |
| Cursor overlay helper (`tinycomputer-cursor-overlay`) | `crates/tinycomputer-cursor/src/screen/process.rs` | Unsupported. Draws on screen, reads commands on stdin, opens no network. Optional; absent means no drawn cursor. |
| Accessibility helper (Swift, macOS) | `crates/tinycomputer-accessibility/src/helper/process.rs`, `globe.rs`, `build.rs` | Unsupported. Needs the macOS accessibility grant of the host app, so it cannot be jailed. Opens no network. |
| `osascript`, `open`, `cmd`, `xcrun`, `swiftc` | `crates/tinycomputer-accessibility/src/{focus,paste,permissions}.rs`, `helper/process.rs` | Unsupported. Fixed argv, no model-supplied command line. Local desktop actions only. |
| agent-browser plugins, Lightpanda, Safari/Appium drivers, upgrade | vendor/agent-browser | Not used by this module's launch path; owned by agent-browser. |

## Egress

The module's own outbound traffic is the decision model, planner, rescue and
output model calls (the `jev`, `planner` config routes), made from the host
process, plus whatever sites the browser is told to open. Host-side egress
policy applies to the first. The second is user-directed browsing and is not
restricted by the module (`origins` checks classify pages, they do not block
network).

## Revisit when

agent-browser or the module can pass a launch-time proxy or network namespace
and the module can verify it took effect. Only then add a contract member, in
`tinycomputer-bus`, with a test that the browser's traffic goes through it.
