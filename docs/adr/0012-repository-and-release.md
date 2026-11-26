# 0012: Repository settings, release flow, changelog and versions

- Status: accepted, amended by [0016](0016-settings-as-code.md) (settings as code, two more required checks)
- Date: 2026-10-14

## Context and problem statement

Before 1.0.0 (v0.1.0 was a test release, since withdrawn), the public repository needed settings that keep `main`
green and releases trustworthy, a release flow that works when nobody can push to `main`, and a rule for what counts
as a breaking change.

## Considered options

1. Who must go through pull requests: everyone, or everyone but the owner (a bypass).
2. Required checks "strict" (the branch must be up to date with `main`) or not.
3. Releasing: tag `main` directly, or a version-bump pull request first and the tag after it's merged.

## Decision outcome

Chosen: pull requests with green CI for everyone, not strict, and a two-step release. Applied with `gh api`:

| Setting | Value | Why |
|---|---|---|
| Ruleset "main: PRs with green CI" (default branch, no bypass) | PR required (0 approvals, squash only), required checks `Build (Linux, cargo-xwin)`, `Test (Windows, native)`, `Smoke test the cross-built exe (Windows)` (GitHub Actions app), linear history, signed commits, no force push, no deletion | every change goes through green CI. No bypass, so it applies to the owner too. Not strict: that would cost an extra CI run per merge |
| Ruleset "release tags" (`refs/tags/v*`) | no deletion, update or force push; creation allowed | a published version's tag can't move |
| Releases | immutable (set by the owner) | assets can't be swapped after publishing |
| Fork PR workflows | approval required for all outside contributors | nobody can burn CI minutes or run code on the runners without approval |
| Default `GITHUB_TOKEN` | read-only, can't approve PRs | least privilege; the release job asks for `contents: write` itself |
| Allowed actions | GitHub-owned and `Swatinem/rust-cache@*` | a PR can't pull in arbitrary third-party actions |
| Merging | squash only, PR title and body as the commit, delete branch on merge, auto-merge allowed | one commit per PR on `main` |
| Security | secret scanning with push protection, private vulnerability reporting, Dependabot alerts and security updates | |

**Release flow**, since `main` can't be pushed to:

1. `just bump X.Y.Z`: a branch `release-vX.Y.Z`; `scripts/bump.sh` sets `Cargo.toml` and `Cargo.lock`, turns
   `[Unreleased]` into `[X.Y.Z] - date` with its compare link and refuses when `[Unreleased]` is empty; commit, push,
   pull request.
2. After that pull request is merged: `just release X.Y.Z` checks that `main` is at that version and has the changelog
   section, creates the signed tag `vX.Y.Z` on the commit that set the version (pull requests merged after it wait for
   the next release), pushes it and watches that tag's release run.

`release.yml` checks the tag against `Cargo.toml` and `CHANGELOG.md`, runs CI, uses the changelog section as the
release notes, creates the release as a draft, attaches all assets (with `SHA256SUMS` and a build provenance
attestation), then publishes it. A published immutable release can't take more assets.

**Changelog and versions:** `CHANGELOG.md` follows Keep a Changelog 1.1.0, and versions follow semver. 1.0.0 is the
first public release: the spec format, the CLI and the exit codes are the public API from there on. User-visible
changes get an `[Unreleased]` line in the same pull request.

### Consequences

- Public repositories get unlimited standard GitHub-hosted runner minutes, so the CI limits are about abuse and noise,
  not cost: pull request runs cancel when superseded (`concurrency`), every job has `timeout-minutes`, and Dependabot
  groups its version updates into one pull request per ecosystem and week (at most 3 open; security updates come as
  they're found).
- The `v0.1.0` tag from the withdrawn test release still exists; the tag ruleset protects it like any other `v*` tag.

## More information

Notes from the first releases:

- `just release`'s clean-tree check ignores untracked files (the placeholder files Claude Code's sandbox creates).
- When `gh run watch` at the end of `just release` fails on a network drop, the release workflow itself is
  unaffected: rerun `gh run watch <id>` or check the Actions tab.
- Verified on v0.1.0: all assets match `SHA256SUMS`, ClamAV finds nothing, and
  `gh attestation verify toastback.exe -R gergodrazsdik/toastback` names `release.yml@refs/tags/v0.1.0`.
