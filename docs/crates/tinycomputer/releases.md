# Releases

## Versioning

The whole workspace releases as one version. Every member crate inherits it
with `version.workspace = true` from the root `[workspace.package]`, so
`tinycomputer` and `tinycomputer-bus` are always the same version number even
though only `tinycomputer` is packaged and shipped.

Separately, `CONTRACT_VERSION` in `tinycomputer-bus` (a `(major, minor)` pair,
distinct from the crate version above) tracks the wire contract itself:
adding a member or an optional field is a minor bump, and changing a wire
form, removing a member, or renaming one, including the interface name
itself, is a major bump. The interface was last renamed at contract 2.0, from
`ai.tinyhumans.tinydesktop.Desktop` to today's
`ai.tinyhumans.tinycomputer.Desktop`, when the repository itself was renamed.
[`../../technical/specs/desktop-module-contract.md`](../../technical/specs/desktop-module-contract.md)
is the authoritative history of every contract bump.

Version updates change the root workspace.package version and lockfile in a
reviewed PR; every workspace member inherits that version.

## How a release actually happens

Update the workspace version and lockfile through a PR. Main requires an
up-to-date branch and the shared CI checks, including module E2E and feature
tests on Linux, macOS and Windows. After merging, push a vX.Y.Z tag at that
commit, or dispatch Release with an existing tag.

The shared pipeline verifies that the tag is on main and matches the Cargo
module version. It builds each native/distro archive and verifies the actual
packaged cdylib using the existing verify_module binary before publishing
one immutable release. Archives include the library, modules.toml, LICENSE,
README.md, MODULE.md, the macOS clipboard helper, and macOS/Windows cursor
overlay binaries. checksum.toml covers the module archives.

See the [shared contract](https://github.com/tinyhumansai/.github/blob/main/docs/module-ci-contract.md).
The release workflow never pushes a version-bump commit to main.

## The platform matrix

Seventeen archives per release, checked at the point in the shared release workflow that
insists exactly that many exist before generating `checksum.toml`:

Native runners, verified through `verify_module` on the real OS:

| Platform | Runner | Target |
| --- | --- | --- |
| Ubuntu 22.04 x86_64 | `ubuntu-22.04` | `x86_64-unknown-linux-gnu` |
| Ubuntu 22.04 arm64 | `ubuntu-22.04-arm` | `aarch64-unknown-linux-gnu` |
| Ubuntu 24.04 x86_64 | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` |
| Ubuntu 24.04 arm64 | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| macOS 15 x86_64 | `macos-15-intel` | `x86_64-apple-darwin` |
| macOS 15 arm64 | `macos-15` | `aarch64-apple-darwin` |
| macOS 26 x86_64 | `macos-26-intel` | `x86_64-apple-darwin` |
| macOS 26 arm64 | `macos-26` | `aarch64-apple-darwin` |
| Windows Server 2022 x86_64 | `windows-2022` | `x86_64-pc-windows-msvc` |
| Windows Server 2025 x86_64 | `windows-2025` | `x86_64-pc-windows-msvc` |
| Windows 11 arm64 | `windows-11-arm` | `aarch64-pc-windows-msvc` |

Distro containers, built on an Ubuntu runner but inside the named container
image, and still verified through `verify_module` for that container's own
`.so`:

| Platform | Container |
| --- | --- |
| Fedora 43 x86_64 | `fedora:43` |
| Fedora 43 arm64 | `fedora:43` |
| Fedora 44 x86_64 | `fedora:44` |
| Fedora 44 arm64 | `fedora:44` |
| Arch Linux rolling x86_64 | `archlinux:base-devel` |
| Arch Linux ARM64 | `menci/archlinuxarm:base-devel` |

Every runner also carries a check that the actual `rustc` host target matches
what the matrix entry claims before building anything, so a runner image
change upstream that quietly swaps architectures fails the build rather than
shipping a mislabeled archive.

TinyBus itself is a pinned submodule dependency, `vendor/tinybus`, used to
build and to run its own module-checksum and release-verification tools
during this workflow. It is never itself published as a release asset from
this repository; only `tinycomputer` is.

## Next

[installing-and-loading.md](installing-and-loading.md) covers what to do
with one of these archives once it exists, and
[`../../technical/specs/tinybus-module-release.md`](../../technical/specs/tinybus-module-release.md)
is the underlying spec this whole pipeline satisfies.
