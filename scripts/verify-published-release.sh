#!/usr/bin/env bash
# Verify the distributed Ubuntu asset, then download and load it through TinyBus.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
repository="${1:?usage: verify-published-release.sh <repository> <tag> <version>}"
tag="${2:?release tag is required}"
version="${3:?release version is required}"
archive="tinycomputer-${version}-ubuntu-24.04-x86_64.tar.gz"
mkdir -p target
work="$(mktemp -d target/published-release.XXXXXX)"
trap 'rm -rf "$work"' EXIT

gh release download "$tag" --repo "$repository" --dir "$work" \
  --pattern checksum.toml --pattern "$archive"
digest="$(python3 - "$work/checksum.toml" "$archive" <<'PY'
import hashlib
import re
import sys
import tomllib
with open(sys.argv[1], 'rb') as manifest:
    digest = tomllib.load(manifest)[sys.argv[2]]
if not isinstance(digest, str) or not re.fullmatch('[0-9a-f]{64}', digest):
    raise ValueError('archive checksum must be a SHA-256 digest')
with open(sys.argv[1].rsplit('/', 1)[0] + '/' + sys.argv[2], 'rb') as archive:
    if hashlib.file_digest(archive, 'sha256').hexdigest() != digest:
        raise ValueError('published archive checksum mismatch')
print(digest)
PY
)"
# Read only the module member; no archive-controlled paths are extracted.
tar -xOzf "$work/$archive" ./libtinycomputer.so > "$work/libtinycomputer.so"
bash scripts/ci/verify-linux-module-runtime-dependencies.sh "$work/libtinycomputer.so"
cargo run --locked --package tinycomputer-examples --bin verify_github_release -- \
  "https://github.com/${repository}/releases/tag/${tag}" "$archive" "$digest"
