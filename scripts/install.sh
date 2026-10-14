#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# toastback installer for WSL: downloads the release exes into the Windows temp dir, checks them
# against SHA256SUMS and runs `toastback.exe install` for this distro and user.
#
#   curl -fsSL https://github.com/gergodrazsdik/toastback/releases/latest/download/install.sh | bash
#   ... | TOASTBACK_VERSION=v0.1.0 bash          # a specific version
#   ... | bash -s -- --add-to-path                # extra arguments go to `toastback.exe install`
#
# Piped into bash, this script *is* bash's stdin. Everything lives in main(), called on the last line,
# so bash has read the whole script before anything runs, and Windows commands get </dev/null:
# interop forwards stdin, and cmd.exe would otherwise swallow the rest of the script.
set -euo pipefail

die() {
  echo "toastback install: $*" >&2
  exit 1
}

main() {
  local repo="gergodrazsdik/toastback"
  local version="${TOASTBACK_VERSION:-latest}"
  local base
  if [ "$version" = latest ]; then
    base="https://github.com/$repo/releases/latest/download"
  else
    base="https://github.com/$repo/releases/download/$version"
  fi
  # testing: point at another copy of the release assets (curl also takes file:// URLs)
  base="${TOASTBACK_BASE_URL:-$base}"

  [ -n "${WSL_DISTRO_NAME:-}" ] || die "run this inside WSL"
  local cmd
  for cmd in curl sha256sum wslpath cmd.exe; do
    command -v "$cmd" >/dev/null || die "$cmd not found"
  done

  # The Windows temp dir. cmd.exe runs from a Windows path, or it warns about the UNC cwd.
  local cdrive win_temp
  cdrive=$(wslpath -u "C:\\")
  win_temp=$(cd "$cdrive" && cmd.exe /d /c 'echo %TEMP%' </dev/null 2>/dev/null | tr -d '\r')
  [ -n "$win_temp" ] || die "could not find the Windows temp dir"
  tmp="$(wslpath -u "$win_temp")/toastback-install-$$"
  mkdir -p "$tmp"
  trap 'rm -rf "$tmp"' EXIT

  echo "downloading toastback ($version)"
  local f
  for f in toastback.exe toastback-cli.exe SHA256SUMS; do
    curl -fsSL --retry 3 -o "$tmp/$f" "$base/$f" </dev/null || die "download failed: $base/$f"
  done

  # check exactly the two exes; a missing line is an error, not a skip
  local sums
  sums=$(grep -E ' (toastback|toastback-cli)\.exe$' "$tmp/SHA256SUMS" || true)
  [ "$(printf '%s\n' "$sums" | grep -c .)" -eq 2 ] || die "SHA256SUMS doesn't list both exes"
  (cd "$tmp" && printf '%s\n' "$sums" | sha256sum -c --quiet -) || die "checksum mismatch: not installing"

  # install copies the exes to %LOCALAPPDATA%\toastback\bin; the temp copies are deleted on exit
  if ! "$tmp/toastback.exe" install --wsl "$WSL_DISTRO_NAME" --distro "$WSL_DISTRO_NAME" \
    --user "$(id -un)" "$@" </dev/null; then
    die "toastback.exe failed or Windows refused to start it. An \"Invalid argument\" error usually means
  Windows App Control blocked it: Smart App Control (Windows Security > App & browser control) blocks
  unsigned apps it doesn't know yet. See https://github.com/$repo#limitations"
  fi

  cat <<'EOF'

next steps:
  toastback -t hello -m world     # a first notification
  toastback doctor                # check the installation
EOF
}

main "$@"
