#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# toastback installer for WSL: downloads the release exes into the Windows temp dir, checks them
# against SHA256SUMS and runs `toastback.exe install` for this distro and user.
#
# Download it with SHA256SUMS from the release, check it (sha256sum -c --ignore-missing SHA256SUMS), read it, run it:
#   bash install.sh                               # the latest release
#   TOASTBACK_VERSION=v1.0.0 bash install.sh      # a specific version
#   bash install.sh --add-to-path                 # extra arguments go to `toastback.exe install`
#
# If it's piped into bash anyway, the script *is* bash's stdin. So everything lives in main(), called on the last
# line, so bash has read the whole script before anything runs, and Windows commands get </dev/null: interop
# forwards stdin, and cmd.exe would otherwise swallow the rest of the script.
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
  for cmd in curl sha256sum wslpath cmd.exe iconv; do
    command -v "$cmd" >/dev/null || die "$cmd not found"
  done

  # The Windows temp dir. cmd.exe runs from a Windows path, or it warns about the UNC cwd. /u: output
  # in UTF-16 (else it's the OEM code page, which mangles names like Jörg); /v:on and !TEMP!: expanded
  # after parsing, so an & in the path stays text.
  local cdrive win_temp
  cdrive=$(wslpath -u "C:\\")
  win_temp=$(cd "$cdrive" && cmd.exe /d /u /v:on /c 'echo(!TEMP!' </dev/null 2>/dev/null |
    iconv -f UTF-16LE -t UTF-8 | tr -d '\r\n') || win_temp=
  [ -n "$win_temp" ] || die "could not find the Windows temp dir (does running cmd.exe from WSL work?)"
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

  # This distro and user, unless the arguments say otherwise (`--wsl all`, `--user`): passing an
  # option twice is an error. (The ${a[@]+...} form: bash < 4.4 calls an empty array unbound.)
  local own=() arg wsl=1 distro=1 user=1
  for arg in "$@"; do
    case "$arg" in
      --wsl | --wsl=*) wsl= ;;
      --distro | --distro=*) distro= ;;
      --user | --user=*) user= ;;
    esac
  done
  [ -z "$wsl" ] || own+=(--wsl "$WSL_DISTRO_NAME")
  [ -z "$distro" ] || own+=(--distro "$WSL_DISTRO_NAME")
  [ -z "$user" ] || own+=(--user "$(id -un)")

  # install copies the exes to %LOCALAPPDATA%\toastback\bin; the temp copies are deleted on exit
  chmod +x "$tmp/toastback.exe" "$tmp/toastback-cli.exe" # drvfs mounted with `metadata` needs it
  if ! "$tmp/toastback.exe" install ${own[@]+"${own[@]}"} "$@" </dev/null; then
    # WSL reports a start that Windows refused as "<exe>: Invalid argument" with exit code 1, the
    # same code as a failed install, so the hint can only be conditional
    die "toastback.exe install failed (see the message above). If it says \"Invalid argument\", Windows
  refused to start it: Smart App Control (Windows Security > App & browser control) blocks unsigned apps
  it doesn't know yet. See https://github.com/$repo#limitations"
  fi

  case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *)
      echo
      echo "note: ~/.local/bin isn't on PATH in this shell yet. A new terminal (or \`exec bash -l\`) usually picks it"
      echo "      up now that it exists. If not, add it to your shell profile."
      ;;
  esac
  cat <<'EOF'

next steps:
  toastback -t hello -m world     # a first notification
  toastback doctor                # check the installation
EOF
}

main "$@"
