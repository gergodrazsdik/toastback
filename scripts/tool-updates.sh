#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Reports which linter versions pinned outside Dependabot's reach have newer releases, and when
# each update is due: a release is taken once it is 7 days old, the same cooldown Dependabot keeps
# for the actions. Read-only: it changes nothing.
#
# Usage: scripts/tool-updates.sh
#
# The pins live in scripts/install-ci-tools.sh (release binaries, with their digests) and in
# .github/workflows/lint.yml (PyPI and npm versions), and cargo-xwin's in .github/workflows/ci.yml.
# Updating one is a pull request that changes the version, URL and digest together; take the
# digest from GitHub (`gh release view <tag> -R <repo> --json assets`).
# Needs curl and jq; reads the GitHub API (GITHUB_TOKEN or GH_TOKEN raises its rate limit), PyPI
# and the npm registry.
set -euo pipefail

cooldown_days=7
root=$(cd "$(dirname "$0")/.." && pwd)
installer="$root/scripts/install-ci-tools.sh"
workflow="$root/.github/workflows/lint.yml"
ci_workflow="$root/.github/workflows/ci.yml"

get() { # <url>: the response body, with the GitHub token for api.github.com
  local token=${GITHUB_TOKEN:-${GH_TOKEN:-}}
  if [[ -n $token && $1 == https://api.github.com/* ]]; then
    curl -fsSL --retry 3 -H "Authorization: Bearer $token" "$1"
  else
    curl -fsSL --retry 3 "$1"
  fi
}

# Stable releases of a source as a JSON array of {v, date}.
releases() { # <source>: github:<owner/repo>, pypi:<package> or npm:<package>
  local kind=${1%%:*} name=${1#*:}
  case $kind in
    github)
      get "https://api.github.com/repos/$name/releases?per_page=50" |
        jq '[.[] | select((.draft or .prerelease) | not) | {v: (.tag_name | ltrimstr("v")), date: .published_at}]'
      ;;
    pypi)
      get "https://pypi.org/pypi/$name/json" |
        jq '[.releases | to_entries[] | select(.value | length > 0) | {v: .key, date: .value[0].upload_time_iso_8601}]'
      ;;
    npm)
      get "https://registry.npmjs.org/$name" |
        jq '[.time | to_entries[] | select(.key | test("^[0-9]+\\.[0-9]+\\.[0-9]+$")) | {v: .key, date: .value}]'
      ;;
  esac
}

# The version a locally installed tool reports: "-" when it isn't installed, "?" when it doesn't say.
local_version() { # <command>
  local out
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "-"
    return
  fi
  out=$({ "$1" --version || "$1" version; } 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1 || true)
  echo "${out:-?}"
}

# The pins: name|source|pinned version
pins=()
while IFS='|' read -r name url _; do
  name=${name#*\"}
  repo=$(sed -E 's#^https://github.com/([^/]+/[^/]+)/releases/download/.*#\1#' <<<"$url")
  tag=$(sed -E 's#^.*/releases/download/([^/]+)/.*#\1#' <<<"$url")
  pins+=("$name|github:$repo|${tag#v}")
done < <(grep -E '^  "[a-z-]+\|https://' "$installer")

env_pin() { # <variable> [<workflow>]: its value in the lint workflow, or the one given
  sed -nE "s/^ *$1: \"?([^\" ]+)\"?.*/\1/p" "${2:-$workflow}" | head -n 1
}
pins+=("yamllint|pypi:yamllint|$(env_pin YAMLLINT_VERSION)")
pins+=("zizmor|pypi:zizmor|$(env_pin ZIZMOR_VERSION)")
pins+=("cargo-xwin|pypi:cargo-xwin|$(env_pin CARGO_XWIN_VERSION "$ci_workflow")")
pins+=("markdownlint-cli2|npm:markdownlint-cli2|$(env_pin MARKDOWNLINT_CLI2_VERSION)")

# shellcheck disable=SC2016  # jq variables, not shell expansions
report='
  def ver: split(".") | map(tonumber? // 0);
  def epoch: sub("\\.[0-9]+"; "") | sub("\\+00:00$"; "Z") | fromdateiso8601;
  def day: todate[0:10];
  ($pinned | ver) as $p
  | [.[] | select(.v | test("^[0-9]+(\\.[0-9]+)*$")) | select((.v | ver) > $p)
      | . + {due: ((.date | epoch) + $days * 86400)}] as $newer
  | ($newer | max_by(.v | ver)) as $newest
  | ([$newer[] | select(.due <= now)] | max_by(.v | ver)) as $take
  | if $newest == null then {newest: $pinned, status: "current"}
    else {
      newest: "\($newest.v) (\($newest.date | epoch | day))",
      status: (
        if $take == null then "\($newest.v) due from \($newest.due | day)"
        elif $take.v == $newest.v then "UPDATE to \($take.v) (due since \($take.due | day))"
        else "UPDATE to \($take.v) (due since \($take.due | day)); \($newest.v) due from \($newest.due | day)"
        end)
    } end
  | "\(.newest)\t\(.status)"'

printf '%-22s %-9s %-9s %-24s %s\n' "Tool" "Pinned" "Local" "Newest (released)" "Status ($cooldown_days-day cooldown)"
for entry in "${pins[@]}"; do
  IFS='|' read -r name source pinned <<<"$entry"
  if ! json=$(releases "$source"); then
    printf '%-22s %-9s %-9s %s\n' "$name" "$pinned" "$(local_version "$name")" "could not read the releases of $source"
    continue
  fi
  IFS=$'\t' read -r newest status < <(jq -r --arg pinned "$pinned" --argjson days "$cooldown_days" "$report" <<<"$json")
  printf '%-22s %-9s %-9s %-24s %s\n' "$name" "$pinned" "$(local_version "$name")" "$newest" "$status"
done

