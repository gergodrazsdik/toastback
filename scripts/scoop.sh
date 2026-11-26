#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Scoop packaging, driven from WSL with the Windows scoop. Run from the repository root; the `just scoop-*` recipes
# wrap it. See docs/adr/0020-scoop.md.
#
#   scripts/scoop.sh setup               # check that scoop works from here
#   scripts/scoop.sh manifest VERSION    # render the manifest of a published release (for the bucket), check it
#   scripts/scoop.sh test-local          # install the local build through scoop, update it, uninstall it
set -euo pipefail

repo=https://github.com/gergodrazsdik/toastback
template=packaging/scoop/toastback.json
schema=https://raw.githubusercontent.com/ScoopInstaller/Scoop/master/schema.json
port=8736
probe=scoop-update-probe

die() {
  echo "scoop: $*" >&2
  exit 1
}

# PowerShell started from WSL inherits the Windows PATH from when WSL started: read it fresh, so a scoop installed
# since (and its shims) are found.
# shellcheck disable=SC2016  # literal $ for PowerShell
fresh_path='$env:Path = [Environment]::GetEnvironmentVariable("Path", "User") + ";" + [Environment]::GetEnvironmentVariable("Path", "Machine")'

# Run PowerShell code with the fresh PATH.
ps() {
  powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$fresh_path; $*" </dev/null
}

# Run a scoop command; scoop is a PowerShell script.
scoop() {
  ps "scoop $*"
}

need_tools() {
  local t
  for t in powershell.exe cmd.exe wslpath iconv jq; do
    command -v "$t" >/dev/null || die "$t not found (does Windows interop work here?)"
  done
  scoop --version >/dev/null 2>&1 || die "scoop not found on Windows: see https://scoop.sh"
}

# A Windows environment variable, as a WSL path (as in winget.sh).
win_env_dir() {
  local cdrive value
  cdrive=$(wslpath -u "C:\\")
  value=$(cd "$cdrive" && cmd.exe /d /u /v:on /c "echo(!$1!" </dev/null 2>/dev/null |
    iconv -f UTF-16LE -t UTF-8 | tr -d '\r\n') || value=
  [ -n "$value" ] || die "could not read %$1%"
  wslpath -u "$value"
}

reg_exists() {
  cmd.exe /d /c "reg query $1" >/dev/null 2>&1 </dev/null
}

this_distro_is_default() {
  wsl.exe -l -v </dev/null | iconv -f UTF-16LE -t UTF-8 | tr -d '\r' |
    grep -q "^\* *${WSL_DISTRO_NAME:-unknown} "
}

# Is toast $1 in the notification center?
in_center() {
  powershell.exe -NoProfile -Command "[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]; @([Windows.UI.Notifications.ToastNotificationManager]::History.GetHistory('Toastback') | Where-Object { \$_.Tag -eq '$1' }).Count" </dev/null |
    tr -d '\r' | grep -qx 1
}

# render VERSION URL SHA256 OUT: write the manifest
render() {
  sed -e "s|__VERSION__|$1|g" -e "s|__URL__|$2|g" -e "s|__SHA256__|$3|g" "$template" >"$4"
  jq . "$4" >/dev/null || die "the manifest isn't valid JSON"
}

check_schema() {
  scripts/with-tool.sh check-jsonschema --schemafile "$schema" "$1"
}

cmd_setup() {
  need_tools
  echo "scoop $(scoop --version 2>/dev/null | tr -d '\r' | sed -n 's/.*\(v[0-9][0-9.]*\).*/\1/p' | head -1)"
  echo "ok: scoop works from WSL"
}

# The manifest of a published release, with the hash from its SHA256SUMS, to dist/scoop/toastback.json.
cmd_manifest() {
  local version=$1 sha out=dist/scoop/toastback.json
  sha=$(curl -fsSL "$repo/releases/download/v$version/SHA256SUMS" </dev/null |
    sed -n 's/^\([0-9a-f]\{64\}\)  *toastback-x86_64\.zip$/\1/p')
  [ -n "$sha" ] || die "no hash for toastback-x86_64.zip in the v$version release"
  mkdir -p "$(dirname "$out")"
  render "$version" "$repo/releases/download/v$version/toastback-x86_64.zip" "$sha" "$out"
  check_schema "$out"
  echo "manifest: $out (copy it to the bucket's bucket/toastback.json)"
}

# Install, update and uninstall the local build through scoop, from a manifest whose URL is served here.
cmd_test_local() {
  local version zip sha win_dir manifest bin shim=""
  need_tools
  version=$(sed -n '/^\[package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' Cargo.toml)
  bin="$(win_env_dir LOCALAPPDATA)/toastback/bin"
  serve=$(mktemp -d) # global: the EXIT trap runs after this function returns
  zip="$serve/toastback-x86_64.zip"
  scripts/package.sh target/x86_64-pc-windows-msvc/release "$zip"
  sha=$(sha256sum "$zip" | cut -d' ' -f1)
  # scoop reads the manifest from a Windows path
  win_dir="$(win_env_dir TEMP)/toastback-scoop"
  rm -rf "$win_dir"
  mkdir -p "$win_dir"
  manifest="$win_dir/toastback.json"
  render "$version" "http://localhost:$port/toastback-x86_64.zip" "$sha" "$manifest"
  check_schema "$manifest"

  # WSL's localhost forwarding makes the server reachable from Windows
  python3 -m http.server "$port" --bind 127.0.0.1 --directory "$serve" >/dev/null 2>&1 &
  server=$!
  trap 'kill "$server" 2>/dev/null || true; rm -rf "$serve"' EXIT
  sleep 1
  kill -0 "$server" 2>/dev/null || die "could not serve on port $port"

  echo "--- removing the current install, if any (logs stay)"
  scoop uninstall toastback >/dev/null 2>&1 || true
  if [ -x "$bin/toastback-cli.exe" ]; then
    "$bin/toastback-cli.exe" uninstall </dev/null || die "uninstall failed"
    sleep 4
  fi

  echo "--- scoop install"
  scoop install "'$(wslpath -w "$manifest")'" || die "scoop install failed"
  [ -x "$bin/toastback-cli.exe" ] || die "not installed in $bin"
  reg_exists 'HKCU\Software\Classes\toastback' || die "no toastback:// handler"
  if this_distro_is_default; then
    shim="$HOME/.local/bin/toastback"
    grep -q '# toastback WSL shim' "$shim" || die "no shim in the default distro: $shim"
  else
    echo "note: ${WSL_DISTRO_NAME:-this distro} isn't the default distro, so the shim isn't checked"
  fi
  ps "toastback-cli --version" || die "toastback-cli isn't on PATH"
  "$bin/toastback-cli.exe" doctor </dev/null || die "doctor reports a problem"

  echo "--- scoop update (the old version's uninstall hook must not uninstall)"
  "$bin/toastback-cli.exe" --id "$probe" -t "scoop update probe" -m "should survive the update" </dev/null >/dev/null
  in_center "$probe" || die "the probe toast didn't show"
  scoop update toastback --force || die "scoop update failed"
  in_center "$probe" || die "the update uninstalled toastback (the probe toast is gone)"
  "$bin/toastback-cli.exe" remove "$probe" </dev/null >/dev/null
  "$bin/toastback-cli.exe" doctor </dev/null || die "doctor reports a problem after the update"

  echo "--- scoop uninstall"
  scoop uninstall toastback || die "scoop uninstall failed"
  sleep 4 # the exe deletes its own folder a moment after exiting
  [ ! -e "$bin" ] || die "left behind: $bin"
  [ -z "$shim" ] || [ ! -e "$shim" ] || die "left behind: $shim"
  if reg_exists 'HKCU\Software\Classes\toastback'; then
    die "left behind: the toastback:// handler"
  fi
  if reg_exists 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Toastback'; then
    die "left behind: the Apps & Features entry"
  fi
  echo "--- passed: installed, updated and uninstalled through scoop"
}

case "${1:-}" in
  setup) cmd_setup ;;
  test-local) cmd_test_local ;;
  manifest)
    [[ ${2:-} =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "usage: scoop.sh manifest X.Y.Z"
    cmd_manifest "$2"
    ;;
  *)
    sed -n '3,9p' "$0" >&2
    exit 2
    ;;
esac
