#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the release zip from a directory holding toastback.exe and toastback-cli.exe. Run from the repository root.
# The release workflow and `just winget-test` both use it, so a local test installs what a release ships.
#
#   scripts/package.sh target/x86_64-pc-windows-msvc/release dist/toastback-x86_64.zip
set -euo pipefail

[ $# -eq 2 ] || {
  echo "usage: $0 EXE_DIR OUT_ZIP" >&2
  exit 2
}
exe_dir=$1
mkdir -p "$(dirname "$2")"
out=$(realpath "$2")

pkg=$(mktemp -d)
trap 'rm -rf "$pkg"' EXIT
mkdir -p "$pkg/examples" "$pkg/res"
cp "$exe_dir/toastback.exe" "$exe_dir/toastback-cli.exe" README.md LICENSE "$pkg/"
cp examples/* "$pkg/examples/"
cp res/icon.png "$pkg/res/"
rm -f "$out"
(cd "$pkg" && zip -qr "$out" .)
