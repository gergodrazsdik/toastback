# 0016: Repository settings as code

- Status: accepted
- Date: 2026-11-26

## Context and problem statement

The repository settings and rulesets ([0012](0012-repository-and-release.md)) were applied by hand with `gh api`
and recorded only as a table. Nothing showed when a setting drifted, a fork couldn't reproduce them, and the new
`lint` and `zizmor` checks ([0015](0015-pinned-ci.md)) had to become required checks.

## Considered options

1. Keep applying them by hand and updating the table in 0012.
2. Keep the rulesets as JSON in `.github/rulesets/`, and the rest in a script that applies them or reads them back
   and reports differences.

## Decision outcome

Chosen option: 2. `scripts/github-settings.sh` (`just github-settings <owner/repo>`, maintainer only) applies the
settings; `just github-settings-check <owner/repo>` reads them back and exits 1 on a difference. It holds no
personal data, so it works for a fork; `--no-required-checks` leaves the required checks out until a new repo's
workflows have run once.

The script encodes 0012's settings, with these changes:

- **Required checks** on `main` add `lint` and `zizmor`.
- **SHA pinning required** for actions (GitHub's Actions setting), now that every action is pinned.
- **"Update branch"** is offered on pull requests (`allow_update_branch`), as CONTRIBUTING.md describes.
- **Topics** describe the project: windows, wsl, toast-notifications, notifications, cli, rust.
- **Labels** are the current set, including the three the issue templates use (bug, enhancement, question).

### Consequences

- A settings change is a pull request that changes the script or a ruleset file; the maintainer applies it after
  the merge and checks it with `just github-settings-check`.
- 0012's table stays as the record of why each setting exists.
