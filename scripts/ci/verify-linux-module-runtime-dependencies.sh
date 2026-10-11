#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! -f "$1" ]]; then
  echo "usage: $0 <linux-module>" >&2
  exit 2
fi

if ! command -v ldd >/dev/null 2>&1; then
  echo "ldd is required to verify Linux module runtime dependencies" >&2
  exit 2
fi

dependencies="$(ldd "$1")"
if unresolved="$(grep -F 'not found' <<<"$dependencies")"; then
  echo "Linux module has unresolved runtime dependencies:" >&2
  echo "$unresolved" >&2
  exit 1
fi

echo "All Linux module runtime dependencies resolve."
