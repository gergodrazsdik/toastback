#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prepare release VERSION: set it in Cargo.toml (and Cargo.lock), and turn CHANGELOG.md's [Unreleased] section
# into "[VERSION] - today" with its compare link. Run from the repository root; `just bump VERSION` wraps it.
#
#   scripts/bump.sh 1.2.0
set -euo pipefail

repo_url="https://github.com/gergodrazsdik/toastback"

die() {
  echo "bump: $*" >&2
  exit 1
}

version=${1:-}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "usage: scripts/bump.sh VERSION (like 1.2.0)"
if [ ! -f CHANGELOG.md ] || [ ! -f Cargo.toml ]; then
  die "run from the repository root"
fi
! grep -q "^## \[$version\]" CHANGELOG.md || die "CHANGELOG.md already has a [$version] section"

# the [Unreleased] section must say what changed
unreleased=$(awk '/^## \[Unreleased\]/ { f = 1; next } /^## \[/ { f = 0 } f && NF' CHANGELOG.md)
[ -n "$unreleased" ] || die "nothing under [Unreleased] in CHANGELOG.md: describe the changes first"

# the newest released version, for the compare link
prev=$(grep -m1 -oE '^## \[[0-9]+\.[0-9]+\.[0-9]+\]' CHANGELOG.md | tr -d '#[] ' || true)
if [ -n "$prev" ]; then
  link="$repo_url/compare/v$prev...v$version"
else
  link="$repo_url/releases/tag/v$version"
fi
today=$(date -u +%F)

sed -i "0,/^## \[Unreleased\]\$/s//## [Unreleased]\n\n## [$version] - $today/" CHANGELOG.md
sed -i "s|^\[unreleased\]: .*|[unreleased]: $repo_url/compare/v$version...HEAD\n[$version]: $link|" CHANGELOG.md
grep -q "^\[$version\]: " CHANGELOG.md || die "no [unreleased]: link line to update in CHANGELOG.md"

sed -i "/^\[package\]/,/^\[/ s/^version = \".*\"/version = \"$version\"/" Cargo.toml
grep -q "^version = \"$version\"\$" Cargo.toml || die "couldn't set the version in Cargo.toml"
cargo update --workspace --offline --quiet # the package's own version in Cargo.lock

echo "bumped to $version: review with 'git diff', then commit and open a PR"
