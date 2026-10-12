#!/usr/bin/env bash
set -euo pipefail

if command -v dnf >/dev/null 2>&1; then
  dnf install -y gcc gcc-c++ cmake curl git gzip jq make openssl-devel perl tar pkgconf-pkg-config alsa-lib-devel libxdo-devel
elif command -v pacman >/dev/null 2>&1; then
  # Nested Arch containers cannot apply Landlock filesystem rules. Keep
  # pacman's syscall sandbox enabled and disable only its filesystem sandbox.
  pacman -Syu --disable-sandbox-filesystem --noconfirm base-devel cmake curl git gzip jq openssl perl alsa-lib xdotool
else
  echo "unsupported distro package manager" >&2
  exit 2
fi
