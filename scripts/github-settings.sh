#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Applies toastback's GitHub repository settings, or reads them back and reports differences.
#
# Usage: scripts/github-settings.sh [--check] [--no-required-checks] <owner/repo>
#   --check               only read the settings and report differences (exit 1 if any)
#   --no-required-checks  leave the required status checks out of the rulesets; use it on a brand-new
#                         repo (or fork) until the CI and lint workflows have run once
#
# Covers: general and merge settings, security features, Actions permissions, immutable releases,
# labels, topics and the rulesets in .github/rulesets/ (ADR 0012, ADR 0015). It holds no personal data, so it works for any fork.
# Needs gh (authenticated as a repository admin) and jq. Idempotent: run it again at any time.
set -euo pipefail

check_only=false
required_checks=true
repo=
for arg in "$@"; do
  case $arg in
    --check) check_only=true ;;
    --no-required-checks) required_checks=false ;;
    -h | --help)
      sed -n '3,12p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    -*)
      echo "unknown option: $arg" >&2
      exit 2
      ;;
    *) repo=$arg ;;
  esac
done
if [[ ! $repo =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  echo "usage: $0 [--check] [--no-required-checks] <owner/repo>" >&2
  exit 2
fi

root=$(cd "$(dirname "$0")/.." && pwd)
api="repos/$repo"
diffs=0

# ---------------------------------------------------------------------------------------------
# Desired state

# Squash only: a squash commit takes the pull request's title and description, and GitHub signs it,
# which the `main` ruleset's signed-commits rule needs.
general='{
  "has_issues": true, "has_wiki": false, "has_projects": false, "has_discussions": false,
  "allow_squash_merge": true, "allow_rebase_merge": false, "allow_merge_commit": false,
  "squash_merge_commit_title": "PR_TITLE", "squash_merge_commit_message": "PR_BODY",
  "allow_auto_merge": true, "allow_update_branch": true, "delete_branch_on_merge": true,
  "web_commit_signoff_required": false
}'

# Actions allowed besides GitHub's own (actions/*, github/*): every third-party action the
# workflows use, by repository.
selected_actions='{
  "github_owned_allowed": true, "verified_allowed": false,
  "patterns_allowed": ["Swatinem/rust-cache@*"]
}'

topics='["windows","wsl","toast-notifications","notifications","cli","rust"]'

# name|colour|description; the issue templates use bug, enhancement and question.
labels=(
  "bug|d73a4a|Something isn't working"
  "enhancement|a2eeef|New feature or request"
  "question|d876e3|Further information is requested"
  "documentation|0075ca|Improvements or additions to documentation"
  "accessibility|f143ab|Barrier affecting people with disabilities"
  "duplicate|cfd3d7|This issue or pull request already exists"
  "invalid|e4e669|This doesn't seem right"
  "wontfix|ffffff|This will not be worked on"
  "good first issue|7057ff|Good for newcomers"
  "help wanted|008672|Extra attention is needed"
  "dependencies|0366d6|Pull requests that update a dependency file"
  "rust|000000|Pull requests that update rust code"
)
# Labels to remove when present.
obsolete_labels=()

ruleset_json() { # <file>: the ruleset to apply, minus required checks when asked
  if [[ $required_checks == true ]]; then
    jq -c . "$1"
  else
    jq -c '.rules |= map(select(.type != "required_status_checks"))' "$1"
  fi
}

# ---------------------------------------------------------------------------------------------
# Helpers

# Runs gh, retrying when GitHub's API times out or drops the connection. Other failures come back
# at once with gh's exit status. Every call below goes through it.
gh() {
  local attempt err status
  err=$(mktemp)
  for attempt in 1 2 3 4 5; do
    status=0
    command gh "$@" 2>"$err" || status=$?
    if [[ $status -eq 0 ]] || ! grep -qiE 'timeout|connection (reset|refused)|EOF' "$err"; then
      cat "$err" >&2
      rm -f "$err"
      return "$status"
    fi
    echo "GitHub didn't answer (try $attempt of 5); retrying." >&2
    sleep 5
  done
  cat "$err" >&2
  rm -f "$err"
  return "$status"
}

report() { # <name> <ok:true|false> <detail>
  if [[ $2 == true ]]; then
    printf '  ok    %s\n' "$1"
  else
    printf '  DIFF  %s: %s\n' "$1" "$3"
    diffs=$((diffs + 1))
  fi
}

put() { # <method> <path> <json>
  printf '%s' "$3" | gh api --method "$1" "$2" --input - --silent
}

# jq: is every key and value of $want present in $have (arrays compared element by element)?
# shellcheck disable=SC2016  # jq variables, not shell expansions
subset='def sub($w; $h):
  if ($w|type) == "object" then ($h|type) == "object" and all($w|keys[]; . as $k | sub($w[$k]; $h[$k]))
  elif ($w|type) == "array" then ($h|type) == "array" and ($w|length) == ($h|length)
    and all(range(0; $w|length); sub($w[.]; $h[.]))
  else $w == $h end;
def norm: .rules |= (sort_by(.type) | map(
  if .type == "required_status_checks" then .parameters.required_status_checks |= sort_by(.context)
  elif .type == "pull_request" then .parameters.allowed_merge_methods |= sort else . end));'

# ---------------------------------------------------------------------------------------------
# Apply

apply() {
  echo "Applying settings to $repo"

  echo "- general and merge settings"
  put PATCH "$api" "$general"

  echo "- security: private vulnerability reporting, Dependabot, secret scanning, push protection"
  gh api --method PUT "$api/private-vulnerability-reporting" --silent
  gh api --method PUT "$api/vulnerability-alerts" --silent
  gh api --method PUT "$api/automated-security-fixes" --silent
  put PATCH "$api" '{"security_and_analysis": {
    "secret_scanning": {"status": "enabled"},
    "secret_scanning_push_protection": {"status": "enabled"}}}'

  echo "- Actions: read-only token, no PR creation or approval, selected actions, SHA pinning, fork PR approval"
  put PUT "$api/actions/permissions/workflow" \
    '{"default_workflow_permissions": "read", "can_approve_pull_request_reviews": false}'
  if ! put PUT "$api/actions/permissions" \
    '{"enabled": true, "allowed_actions": "selected", "sha_pinning_required": true}'; then
    echo "  warning: SHA pinning policy not accepted; pinact and zizmor in 'just lint' still enforce it" >&2
    put PUT "$api/actions/permissions" '{"enabled": true, "allowed_actions": "selected"}'
  fi
  put PUT "$api/actions/permissions/selected-actions" "$selected_actions"
  put PUT "$api/actions/permissions/fork-pr-contributor-approval" \
    '{"approval_policy": "all_external_contributors"}'

  echo "- immutable releases"
  if ! gh api --method PUT "$api/immutable-releases" --silent; then
    echo "  warning: immutable releases could not be enabled" >&2
  fi

  echo "- topics"
  put PUT "$api/topics" "$(jq -cn --argjson n "$topics" '{names: $n}')"

  echo "- labels"
  local existing entry name color desc body
  existing=$(gh api --paginate "$api/labels" --jq '.[].name')
  for entry in "${labels[@]}"; do
    IFS='|' read -r name color desc <<<"$entry"
    body=$(jq -cn --arg n "$name" --arg c "$color" --arg d "$desc" '{name: $n, new_name: $n, color: $c, description: $d}')
    if grep -Fxq "$name" <<<"$existing"; then
      put PATCH "$api/labels/$(jq -rn --arg n "$name" '$n|@uri')" "$body"
    else
      put POST "$api/labels" "$(jq -c 'del(.new_name)' <<<"$body")"
    fi
  done
  for name in "${obsolete_labels[@]+"${obsolete_labels[@]}"}"; do
    if grep -Fxq "$name" <<<"$existing"; then
      gh api --method DELETE "$api/labels/$(jq -rn --arg n "$name" '$n|@uri')" --silent
    fi
  done

  echo "- rulesets"
  local file rs id
  for file in "$root"/.github/rulesets/*.json; do
    rs=$(ruleset_json "$file")
    name=$(jq -r .name <<<"$rs")
    if [[ $(jq '.rules | length' <<<"$rs") -eq 0 ]]; then
      echo "  $name: skipped (no rules besides required checks)"
      continue
    fi
    id=$(gh api "$api/rulesets" --jq ".[] | select(.name == \"$name\") | .id")
    if [[ -n $id ]]; then
      put PUT "$api/rulesets/$id" "$rs"
    else
      put POST "$api/rulesets" "$rs"
    fi
    echo "  $name"
  done
  echo
}

# ---------------------------------------------------------------------------------------------
# Check

check() {
  echo "Checking settings of $repo"
  local have ok got

  have=$(gh api "$api")
  ok=$(jq --argjson w "$general" "$subset sub(\$w; .)" <<<"$have")
  report "general and merge settings" "$ok" \
    "$(jq -c --argjson w "$general" '. as $h | $w | with_entries(select(.value != $h[.key])) | keys' <<<"$have")"
  # Public repos always allow forking; GitHub only accepts the setting on org-owned private repos.
  got=$(jq -r .allow_forking <<<"$have")
  report "forking allowed" "$got" "allow_forking=$got"
  got=$(jq -c '[.security_and_analysis.secret_scanning.status, .security_and_analysis.secret_scanning_push_protection.status]' <<<"$have")
  report "secret scanning and push protection" "$([[ $got == '["enabled","enabled"]' ]] && echo true || echo false)" "$got"

  got=$(gh api "$api/private-vulnerability-reporting" --jq .enabled)
  report "private vulnerability reporting" "$got" "enabled=$got"
  if gh api "$api/vulnerability-alerts" --silent 2>/dev/null; then got=true; else got=false; fi
  report "Dependabot alerts" "$got" "disabled"
  got=$(gh api "$api/automated-security-fixes" --jq .enabled)
  report "Dependabot security updates" "$got" "enabled=$got"

  got=$(gh api "$api/actions/permissions/workflow" --jq '"\(.default_workflow_permissions) \(.can_approve_pull_request_reviews)"')
  report "Actions token read-only, no PR creation or approval" "$([[ $got == "read false" ]] && echo true || echo false)" "$got"
  got=$(gh api "$api/actions/permissions" --jq '"\(.allowed_actions) \(if has("sha_pinning_required") then .sha_pinning_required else "unsupported" end)"')
  report "Actions: selected actions, SHA pinning required" "$([[ $got == "selected true" ]] && echo true || echo false)" "$got"
  got=$(gh api "$api/actions/permissions/selected-actions")
  ok=$(jq --argjson w "$selected_actions" \
    '.github_owned_allowed == $w.github_owned_allowed and .verified_allowed == $w.verified_allowed
      and ((.patterns_allowed // []) | sort) == ($w.patterns_allowed | sort)' <<<"$got")
  report "allowed actions: GitHub's own plus $(jq -r '.patterns_allowed | join(",")' <<<"$selected_actions")" "$ok" "$(jq -c . <<<"$got")"
  got=$(gh api "$api/actions/permissions/fork-pr-contributor-approval" --jq .approval_policy)
  report "fork PR approval for all external contributors" \
    "$([[ $got == all_external_contributors ]] && echo true || echo false)" "$got"

  got=$(gh api "$api/immutable-releases" --jq .enabled 2>/dev/null || echo unavailable)
  report "immutable releases" "$([[ $got == true ]] && echo true || echo false)" "enabled=$got"

  got=$(gh api "$api/topics" --jq '.names | sort | join(",")')
  report "topics" "$([[ $got == "$(jq -r 'sort | join(",")' <<<"$topics")" ]] && echo true || echo false)" "$got"

  local existing entry name color want_names missing extra
  existing=$(gh api --paginate "$api/labels" --jq '.[] | "\(.name)|\(.color)"')
  missing=()
  for entry in "${labels[@]}"; do
    IFS='|' read -r name color _ <<<"$entry"
    grep -Fxq "$name|$color" <<<"$existing" || missing+=("$name")
  done
  for name in "${obsolete_labels[@]+"${obsolete_labels[@]}"}"; do
    grep -q "^$name|" <<<"$existing" && missing+=("$name (should be removed)")
  done
  report "labels" "$([[ ${#missing[@]} -eq 0 ]] && echo true || echo false)" "$(IFS=,; echo "${missing[*]:-}")"
  want_names=$(printf '%s\n' "${labels[@]}" | cut -d'|' -f1)
  extra=$(cut -d'|' -f1 <<<"$existing" | grep -Fxv -f <(printf '%s\n' "$want_names") | paste -sd, - || true)
  [[ -n $extra ]] && echo "  note  other labels kept: $extra"

  local file want id rs_have
  for file in "$root"/.github/rulesets/*.json; do
    want=$(ruleset_json "$file")
    name=$(jq -r .name <<<"$want")
    if [[ $(jq '.rules | length' <<<"$want") -eq 0 ]]; then
      echo "  skip  ruleset $name (no rules besides required checks)"
      continue
    fi
    id=$(gh api "$api/rulesets" --jq ".[] | select(.name == \"$name\") | .id")
    if [[ -z $id ]]; then
      report "ruleset $name" false "missing"
      continue
    fi
    rs_have=$(gh api "$api/rulesets/$id")
    ok=$(jq -n --argjson w "$want" --argjson h "$rs_have" \
      "$subset (\$w | norm) as \$w | (\$h | norm) as \$h | sub(\$w | del(.name); \$h)")
    report "ruleset $name" "$ok" "differs from $(basename "$file")"
  done

  if [[ $diffs -eq 0 ]]; then
    echo "All settings match."
  else
    echo "$diffs setting(s) differ."
    return 1
  fi
}

if [[ $check_only == false ]]; then
  apply
fi
check
