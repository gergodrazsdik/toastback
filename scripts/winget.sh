#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# winget packaging, driven from WSL with the Windows winget.exe and wingetcreate.exe. Run from the repository root;
# the `just winget-*` recipes wrap it. See docs/decisions/006-winget.md.
#
#   scripts/winget.sh setup              # check the tools and the settings the other commands need
#   scripts/winget.sh manifest VERSION   # render the manifest of a published release, validate it
#   scripts/winget.sh test-local         # install the local build through winget, check it, uninstall it
#   scripts/winget.sh test-release VERSION  # the same with a published release
#   scripts/winget.sh submit VERSION     # open the first winget-pkgs PR (a new package)
#   scripts/winget.sh update VERSION     # open a winget-pkgs PR for a new version
set -euo pipefail

id=Gergo.Drazsdik.Toastback
repo=https://github.com/gergodrazsdik/toastback
templates=packaging/winget
port=8735

die() {
  echo "winget: $*" >&2
  exit 1
}

need_tools() {
  local t
  for t in winget.exe wingetcreate.exe cmd.exe wslpath iconv; do
    command -v "$t" >/dev/null || die "$t not found (does Windows interop work here?)"
  done
}

# A Windows environment variable, as a WSL path. cmd.exe runs from a Windows path, or it warns about the UNC cwd;
# /u: UTF-16 output, /v:on: expanded after parsing, so an & in the path stays text.
win_env_dir() {
  local cdrive value
  cdrive=$(wslpath -u "C:\\")
  value=$(cd "$cdrive" && cmd.exe /d /u /v:on /c "echo(!$1!" </dev/null 2>/dev/null |
    iconv -f UTF-16LE -t UTF-8 | tr -d '\r\n') || value=
  [ -n "$value" ] || die "could not read %$1%"
  wslpath -u "$value"
}

# A directory on the Windows side: winget doesn't take manifests from a \\wsl.localhost path.
win_work_dir() {
  printf '%s/toastback-winget/%s' "$(win_env_dir TEMP)" "$1"
}

# render VERSION URL SHA256 RELEASE_DATE: write the manifest to a Windows dir and print that dir (a WSL path)
render() {
  local version=$1 url=$2 sha=$3 date=$4 dir f name
  dir=$(win_work_dir "$version")
  rm -rf "$dir"
  mkdir -p "$dir"
  for f in "$templates"/*.yaml; do
    name=$(basename "$f")
    sed -e "s|__VERSION__|$version|g" -e "s|__URL__|$url|g" \
      -e "s|__SHA256__|$sha|g" -e "s|__DATE__|$date|g" "$f" >"$dir/$name"
  done
  printf '%s' "$dir"
}

validate() {
  # warnings count as failures too (winget's exit code says so): winget-pkgs' checks are stricter than this one
  winget.exe validate --manifest "$(wslpath -w "$1")" </dev/null || die "the manifest isn't valid"
}

release_url() {
  printf '%s/releases/download/v%s/toastback-x86_64.zip' "$repo" "$1"
}

# The manifest of a published release; prints its dir. Takes the hash from the release's SHA256SUMS.
release_manifest() {
  local version=$1 sha date
  sha=$(curl -fsSL "$repo/releases/download/v$version/SHA256SUMS" </dev/null |
    sed -n 's/^\([0-9a-f]\{64\}\)  *toastback-x86_64\.zip$/\1/p')
  [ -n "$sha" ] || die "no hash for toastback-x86_64.zip in the v$version release"
  # the release's own date, from the changelog
  date=$(sed -n "s/^## \[$version\] - \([0-9-]*\)$/\1/p" CHANGELOG.md)
  [ -n "$date" ] || die "CHANGELOG.md has no dated [$version] section"
  render "$version" "$(release_url "$version")" "${sha^^}" "$date"
}

check_local_manifests_enabled() {
  winget.exe settings export </dev/null | grep -q '"LocalManifestFiles":true' || die "local manifests are off. In an
  elevated (administrator) prompt on Windows, once:  winget settings --enable LocalManifestFiles"
}

reg_exists() {
  cmd.exe /d /c "reg query $1" >/dev/null 2>&1 </dev/null
}

# Is this distro the default one, which a first install from Windows writes the shim to?
this_distro_is_default() {
  wsl.exe -l -v </dev/null | iconv -f UTF-16LE -t UTF-8 | tr -d '\r' |
    grep -q "^\* *${WSL_DISTRO_NAME:-unknown} "
}

# The whole cycle against a manifest in $1: clean slate, install, check, uninstall, check.
cycle() {
  local dir=$1 win_dir bin shim=""
  win_dir=$(wslpath -w "$dir")
  bin="$(win_env_dir LOCALAPPDATA)/toastback/bin"
  validate "$dir"

  echo "--- removing the current install, if any (logs stay)"
  if [ -x "$bin/toastback-cli.exe" ]; then
    "$bin/toastback-cli.exe" uninstall </dev/null || die "uninstall failed"
    sleep 4
  fi

  echo "--- winget install"
  winget.exe install --manifest "$win_dir" --silent --disable-interactivity \
    --accept-package-agreements --accept-source-agreements </dev/null || die "winget install failed"
  [ -x "$bin/toastback-cli.exe" ] || die "not installed in $bin"
  reg_exists 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Toastback' ||
    die "no Apps & Features entry"

  if this_distro_is_default; then
    shim="$HOME/.local/bin/toastback"
    grep -q '# toastback WSL shim' "$shim" || die "no shim in the default distro: $shim"
  else
    echo "note: ${WSL_DISTRO_NAME:-this distro} isn't the default distro, so the shim isn't checked"
  fi

  echo "--- doctor"
  "$bin/toastback-cli.exe" doctor </dev/null || die "doctor reports a problem"
  "$bin/toastback-cli.exe" --version </dev/null

  echo "--- winget sees it"
  winget.exe list --name toastback --disable-interactivity </dev/null || die "winget list doesn't show it"

  echo "--- winget uninstall"
  winget.exe uninstall --manifest "$win_dir" --silent --disable-interactivity </dev/null ||
    die "winget uninstall failed"
  sleep 4 # the exe deletes its own folder a moment after exiting
  [ ! -e "$bin" ] || die "left behind: $bin"
  [ -z "$shim" ] || [ ! -e "$shim" ] || die "left behind: $shim"
  if reg_exists 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Toastback'; then
    die "left behind: the Apps & Features entry"
  fi
  if reg_exists 'HKCU\Software\Classes\toastback'; then
    die "left behind: the toastback:// handler"
  fi
  echo "--- passed: installed, checked and uninstalled through winget"
}

cmd_setup() {
  need_tools
  winget.exe --version </dev/null
  wingetcreate.exe --version </dev/null | sed -n 1p || true # it exits non-zero after printing the version
  check_local_manifests_enabled
  echo "local manifests: on"
  echo "for submit and update: run  wingetcreate token --store  on Windows once, or sign in when it asks"
}

cmd_manifest() {
  local dir
  need_tools
  dir=$(release_manifest "$1")
  validate "$dir"
  echo "manifest: $(wslpath -w "$dir")"
}

cmd_test_local() {
  local version zip dir sha
  need_tools
  check_local_manifests_enabled
  version=$(sed -n '/^\[package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' Cargo.toml)
  serve=$(mktemp -d)
  zip="$serve/toastback-x86_64.zip"
  scripts/package.sh target/x86_64-pc-windows-msvc/release "$zip"
  sha=$(sha256sum "$zip" | cut -d' ' -f1)
  dir=$(render "$version" "http://localhost:$port/toastback-x86_64.zip" "${sha^^}" "$(date +%F)")

  # winget only downloads from a URL, so serve the zip; WSL's localhost forwarding makes it reachable from Windows
  python3 -m http.server "$port" --bind 127.0.0.1 --directory "$serve" >/dev/null 2>&1 &
  server=$!
  trap 'kill "$server" 2>/dev/null || true; rm -rf "$serve"' EXIT
  sleep 1
  kill -0 "$server" 2>/dev/null || die "could not serve on port $port"
  cycle "$dir"
}

cmd_test_release() {
  need_tools
  check_local_manifests_enabled
  cycle "$(release_manifest "$1")"
}

cmd_submit() {
  local dir
  need_tools
  dir=$(release_manifest "$1")
  validate "$dir"
  wingetcreate.exe submit "$(wslpath -w "$dir")" --prtitle "New package: $id version $1" </dev/null
}

cmd_update() {
  local version=$1 date
  need_tools
  date=$(sed -n "s/^## \[$version\] - \([0-9-]*\)$/\1/p" CHANGELOG.md)
  [ -n "$date" ] || die "CHANGELOG.md has no dated [$version] section"
  wingetcreate.exe update "$id" --version "$version" --urls "$(release_url "$version")" \
    --release-date "$date" --release-notes-url "$repo/releases/tag/v$version" \
    --prtitle "New version: $id version $version" --submit </dev/null
}

check_version() {
  [[ ${1:-} =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "usage: winget.sh $2 X.Y.Z"
}

case "${1:-}" in
  setup) cmd_setup ;;
  test-local) cmd_test_local ;;
  manifest | test-release | submit | update)
    check_version "${2:-}" "$1"
    case "$1" in
      manifest) cmd_manifest "$2" ;;
      test-release) cmd_test_release "$2" ;;
      submit) cmd_submit "$2" ;;
      update) cmd_update "$2" ;;
    esac
    ;;
  *)
    sed -n '3,11p' "$0" >&2
    exit 2
    ;;
esac
