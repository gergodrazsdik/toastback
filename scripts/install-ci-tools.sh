#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs the release binaries the lint workflow needs into <bin-dir>, each
# checked against the SHA-256 digest pinned below before it is unpacked.
# Linux x86_64 only: this is for CI runners. Locally, `just doctor` lists the
# tools and how to install them.
#
# Usage: scripts/install-ci-tools.sh <bin-dir>
#
# Update a tool: change its version, URL and digest together. GitHub shows
# each release asset's digest (`gh release view <tag> -R <repo> --json assets`).
# Dependabot doesn't watch these pins: `just tool-updates` says which have newer
# releases and when each update is due.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <bin-dir>" >&2
  exit 2
fi
bin=$1
mkdir -p "$bin"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# name|url|sha256|file name of the binary inside the archive, or "-" when the download is the binary itself
tools=(
  "just|https://github.com/casey/just/releases/download/1.59.0/just-1.59.0-x86_64-unknown-linux-musl.tar.gz|409044617b6f06517c1035e55ca38c8c856ff8b650e41bc2aa1ef82d5903a23e|just"
  "actionlint|https://github.com/rhysd/actionlint/releases/download/v1.7.12/actionlint_1.7.12_linux_amd64.tar.gz|8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8|actionlint"
  "typos|https://github.com/crate-ci/typos/releases/download/v1.52.2/typos-v1.52.2-x86_64-unknown-linux-musl.tar.gz|3b13d1291281d39d01822e54035636487a7a0f228e70097899b1e33aeb0e7e93|typos"
  "editorconfig-checker|https://github.com/editorconfig-checker/editorconfig-checker/releases/download/v4.0.2/editorconfig-checker-linux-amd64.tar.gz|44efaef3ed14c6ab52eb87481d78e5e5c204796dccf26e2f4598a61ba17f8003|editorconfig-checker"
  "gitleaks|https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz|551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb|gitleaks"
  "pinact|https://github.com/suzuki-shunsuke/pinact/releases/download/v5.0.2/pinact_linux_amd64.tar.gz|7f003d8f51e984c2264d2009d430e3e0e87ae404a0e2dc2a9a277b0f731bf5b7|pinact"
  "shellcheck|https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.linux.x86_64.tar.xz|8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198|shellcheck"
)

for entry in "${tools[@]}"; do
  IFS='|' read -r name url sha file <<<"$entry"
  archive="$tmp/$name.archive"
  curl -fsSL --retry 3 -o "$archive" "$url"
  echo "$sha  $archive" | sha256sum --check --strict --quiet
  if [[ $file == - ]]; then
    install -m 0755 "$archive" "$bin/$name"
    echo "installed $name from $url"
    continue
  fi
  mkdir -p "$tmp/$name"
  tar -xf "$archive" -C "$tmp/$name"
  mapfile -t found < <(find "$tmp/$name" -type f -name "$file")
  if [[ ${#found[@]} -ne 1 ]]; then
    echo "error: expected one $file in $url, found ${#found[@]}" >&2
    exit 1
  fi
  install -m 0755 "${found[0]}" "$bin/$name"
  echo "installed $name from $url"
done
