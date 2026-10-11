# .github/ and root configuration

This page covers continuous integration, the release workflow, the coverage
script, the pull request and issue templates, and the root configuration
files that shape every build: `Cargo.toml`, `deny.toml`, and `.env.example`.

## The four contract commands

CI runs exactly these, from the repository root, and a green local run should
mean a green CI run:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets --all-features
cargo test --all-features
```

## .github/workflows/ci.yml

Runs for pull requests and pushes to main. The shared module-ci.yml supplies
the required fmt, clippy, test, coverage, docs, msrv and supply-chain jobs, plus
module-e2e and feature-tests on Ubuntu, macOS and Windows. Every checkout
initializes recursive submodules. Main requires all 13 named shared checks
and an up-to-date branch.

Repository scripts retain the bundled example and transport-free contract
assertion. The loader verifier executes the packaged native library.

Two checks are worth knowing about because they are easy to break by
accident:

- **The contract-crate check.** `crates/tinycomputer-bus` exists so a host can
  depend on the wire types without compiling the module or an async runtime.
  The feature-test script runs `cargo tree -p tinycomputer-bus -e normal,build --prefix
  none` and greps it for `tinybus`, `tokio`, `reqwest`, `ureq`, `hyper`,
  `rusqlite`, `git2`, or `agent-desktop`. A match fails the build. It has to
  be this direction (scoped `-p`, not `-i`) because the inverse form discards
  the `-p` scope and can pass even when `tinycomputer-bus` is the crate at
  fault.
- **Per-file coverage.** `.github/scripts/check-file-coverage.sh` (below)
  enforces 90% line coverage in every source file under `crates/`, not just a
  workspace average.

## .github/workflows/release.yml

Version changes update workspace.package and the lockfile through a reviewed
PR. After merging, push a vX.Y.Z tag at that commit, or dispatch Release with
an existing tag. The shared pipeline builds and loads all 17 native/distro
module archives before creating one immutable release and checksum.toml.

Packaging scripts preserve the macOS clipboard helper and macOS/Windows
cursor overlays. The workflow verifies the reviewed source commit and Cargo
version; it never pushes a version bump directly to main. Full instructions
are in [the release guide](../../crates/tinycomputer/releases.md).

## .github/scripts/check-file-coverage.sh

Run standalone as `.github/scripts/check-file-coverage.sh [minimum] [report-path]`
(defaults: 90, `coverage.json`). It runs `cargo llvm-cov` across the workspace
(excluding `tinycomputer-examples`) with all features, then filters the JSON
report to files under `crates/` (submodules and `worktrees/` are outside that
prefix and excluded on their own), normalizes LLVM's per-monomorphization line
counts down to unique source lines, prints a per-file table, writes a Markdown
table to `$GITHUB_STEP_SUMMARY` when running in CI, and exits non-zero listing
every file under the coverage minimum. On macOS it also excludes
`tinycomputer-desktop/src/desktop/clipboard.rs`, because exercising the
clipboard-clear path locally would erase your real pasteboard; Linux CI has no
pasteboard and covers that branch instead.

Run it yourself with a build of `cargo-llvm-cov` installed
(`cargo install cargo-llvm-cov`, or `cargo llvm-cov --version` to check) if
you want the same per-file numbers CI reports.

## .github/dependabot.yml

Two ecosystems, weekly, capped at 5 open pull requests each, patch-level
updates ignored to keep the queue to changes worth reviewing:

- `cargo` (root `/`): minor and patch bumps are grouped into a single weekly
  pull request; anything else (a major bump) arrives as its own PR.
- `github-actions` (root `/`): all matching updates grouped into one weekly
  pull request.

## .github/PULL_REQUEST_TEMPLATE.md

The checklist every pull request should fill in: a summary, the related
issue (or "None"), API/behavior changes (or "None"), the validation commands
actually run with their outcome, what tests were added or updated, a
documentation note, and a short checklist (single logical change, no new
`#[allow(...)]`/`#[ignore]`/relaxed lint, no secrets in the diff).

## .github/ISSUE_TEMPLATE/

Two structured forms and a routing file:

- `bug_report.yml`: version/commit, Rust toolchain, a minimal reproduction,
  expected versus actual behavior, and free-form context. All but context are
  required.
- `feature_request.yml`: the problem or workflow, the proposed solution,
  alternatives considered, and context. Problem and proposal are required.
- `config.yml`: allows blank issues, and adds a contact link that redirects
  security reports to the security policy instead of a public issue.

## Root configuration files

### Cargo.toml

A virtual workspace (`[workspace]`, no root package): every crate lives
under `crates/`, one directory per package, matched by `members =
["crates/*"]`. `vendor` and `worktrees` are explicitly excluded, because both
hold their own nested Cargo manifests that would otherwise look like
duplicate packages to cargo (each vendored submodule is its own workspace,
and each entry under `worktrees/` is a full `git worktree` checkout of this
same repository).

`[workspace.package]` holds the one shared version, edition (`2024`), MSRV
(`1.89`), license (`GPL-3.0-only`), and repository URL every member inherits
with `field.workspace = true`.

`[workspace.dependencies]` is where every shared dependency is declared once,
including the path dependencies into the four vendored submodules described
in [`docs/project/vendor/README.md`](../vendor/README.md); each entry carries
a comment explaining why the crate is needed. `[workspace.lints.rust]` and
`[workspace.lints.clippy]` set the lint levels every crate opts into with
`[lints] workspace = true`, including `unsafe_code = "forbid"`,
`clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, and
`clippy::pedantic`, all as CI-fatal warnings. A `[profile.release]` section
sets thin LTO, one codegen unit, and stripped debug info for the shipped
module; a `[profile.dev.package.tinycomputer-cursor]` override keeps the
cursor sprite's supersampling fast in dev builds.

### deny.toml

Configuration for `cargo deny check all`, the same check the `supply-chain`
CI job runs. It fails the build on any dependency with a security advisory
or an unmaintained warning (`[advisories]`, with `ignore = []` so an
exemption needs an explicit, commented entry), restricts accepted licenses to
a fixed allow-list including this workspace's own `GPL-3.0-only`
(`[licenses]`), warns on duplicate dependency versions and denies wildcard
version requirements except for internal path dependencies on unpublished
crates (`[bans]`, with the exemption's reasoning spelled out in a comment),
and restricts crate sources to crates.io plus one explicitly allowed git
source, `tinyhumansai/tinytools`, needed only by the lab's optional LLM
author feature (`[sources]`).

### .env.example

Names and documents every environment variable any crate, test, or example
reads, with a placeholder value or a commented-out example and a one-line
explanation of what it is for. It never holds a real value; `.env` (the file
you copy it to) is git-ignored and must never be committed. Variables cover:

- general Rust logging/backtrace toggles (`RUST_LOG`, `RUST_BACKTRACE`)
- the lab (`scripts/lab`): `OPENROUTER_API_KEY`, `TINYCOMPUTER_LAB_MODEL`,
  `TINYCOMPUTER_LAB_SELF_EMAIL`, `TINYCOMPUTER_LAB_JEV_MODEL`,
  `TINYCOMPUTER_MODULE`
- the Jev debug journal: `TINYCOMPUTER_JEV_JOURNAL`
- flow and deliberation strategy: `TINYCOMPUTER_FLOW_STRATEGY`,
  `TINYCOMPUTER_FLOW_DELIBERATION`
- browser perception and launch: `TINYCOMPUTER_BROWSER_PERCEPTION`,
  `TINYCOMPUTER_BROWSER_EXECUTABLE`, `TINYCOMPUTER_BROWSER_ARGS`,
  `TINYCOMPUTER_BROWSER_USER_AGENT`, `TINYCOMPUTER_BROWSER_ENDPOINT`,
  `TINYCOMPUTER_FIXTURE_URL`
- the cursor overlay helper: `TINYCOMPUTER_CURSOR_OVERLAY`
- the task planner and rescues: `TINYCOMPUTER_PLANNER_MODEL`,
  `TINYCOMPUTER_RESCUE_MODEL`, `TASK_RESCUES`
- opting into live browser runs: `TINYCOMPUTER_LIVE_BROWSER`

If you add a new environment variable anywhere in the workspace, add it here
too, in the same commit, with a placeholder rather than a real value.

### rust-toolchain

There is no pinned `rust-toolchain`/`rust-toolchain.toml` file in this
repository. CI installs `stable` (and, for the `msrv` job, whatever version
`rust-version` in `[workspace.package]` names) with `dtolnay/rust-toolchain`;
locally you're expected to have a recent stable toolchain that satisfies that
same MSRV.

## See also

- [`docs/technical/lab.md`](../../technical/lab.md) and
  [`docs/technical/docker-lab.md`](../../technical/docker-lab.md) for running
  things locally the way CI's build steps do
- [`docs/technical/specs/tinybus-module-release.md`](../../technical/specs/tinybus-module-release.md)
  for the release workflow's design rationale
- the repository's `AGENTS.md`/`CLAUDE.md` "Build And Test" and "Releases"
  sections for the human-facing version of this same material
