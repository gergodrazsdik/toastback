#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prepare release VERSION: set it in Cargo.toml and Cargo.lock, and turn CHANGELOG.md's [Unreleased] section
# into "[VERSION] - today" with its compare link. Run from the repository root; `just bump VERSION` wraps it.
# Every check runs before the first edit, so a failure changes nothing. --check runs only the checks.
#
#   scripts/bump.sh [--check] 1.2.0
set -euo pipefail

repo_url="https://github.com/gergodrazsdik/toastback"

die() {
  echo "bump: $*" >&2
  exit 1
}

check_only=false
if [ "${1:-}" = --check ]; then
  check_only=true
  shift
fi
version=${1:-}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "usage: scripts/bump.sh [--check] VERSION (like 1.2.0)"
if [ ! -f CHANGELOG.md ] || [ ! -f Cargo.toml ] || [ ! -f Cargo.lock ]; then
  die "run from the repository root"
fi
! grep -q "^## \[$version\]" CHANGELOG.md || die "CHANGELOG.md already has a [$version] section"
grep -qx '## \[Unreleased\]' CHANGELOG.md || die "CHANGELOG.md has no '## [Unreleased]' line"
grep -q '^\[unreleased\]: ' CHANGELOG.md || die "CHANGELOG.md has no [unreleased]: link line"

# the [Unreleased] section must say what changed
unreleased=$(awk '/^## \[Unreleased\]/ { f = 1; next } /^## \[/ { f = 0 } f && NF' CHANGELOG.md)
[ -n "$unreleased" ] || die "nothing under [Unreleased] in CHANGELOG.md: describe the changes first"

grep -q '^\[package\]' Cargo.toml || die "Cargo.toml has no [package] section"
grep -qx 'name = "toastback"' Cargo.lock || die "Cargo.lock has no toastback package"
if $check_only; then
  exit 0
fi

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
sed -i "/^\[package\]/,/^\[/ s/^version = \".*\"/version = \"$version\"/" Cargo.toml
# the package's own entry in Cargo.lock (cargo update would need the registry, maybe the network)
sed -i "/^name = \"toastback\"\$/{n;s/^version = \".*\"/version = \"$version\"/}" Cargo.lock

grep -q "^## \[$version\] - $today\$" CHANGELOG.md || die "couldn't add the [$version] heading to CHANGELOG.md"
grep -q "^\[$version\]: " CHANGELOG.md || die "couldn't add the [$version] link to CHANGELOG.md"
grep -q "^version = \"$version\"\$" Cargo.toml || die "couldn't set the version in Cargo.toml"
grep -A1 -x 'name = "toastback"' Cargo.lock | grep -qx "version = \"$version\"" ||
  die "couldn't set the version in Cargo.lock"

echo "bumped to $version: review with 'git diff', then commit and open a PR"
