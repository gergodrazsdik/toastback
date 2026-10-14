# 005: GitHub settings, release flow, changelog

Date: 2026-10-14. Applied with `gh api` when preparing 1.0.0 (v0.1.0 was a test release, since withdrawn).

## Repository settings

| Setting | Value | Why |
|---|---|---|
| Ruleset "main: PRs with green CI" (default branch, no bypass) | PR required (0 approvals, squash only), required checks `Build (Linux, cargo-xwin)`, `Test (Windows, native)`, `Smoke test the cross-built exe (Windows)` (GitHub Actions app), linear history, signed commits, no force push, no deletion | every change goes through green CI. No bypass, so it applies to the owner too. Not "strict" (branch needn't be up to date): that would cost an extra CI run per merge |
| Ruleset "release tags" (`refs/tags/v*`) | no deletion, update or force push; creation allowed | a published version's tag can't move |
| Releases | immutable (set by the owner) | assets can't be swapped after publishing |
| Fork PR workflows | approval required for all outside contributors | nobody can burn CI minutes or run code on the runners without approval |
| Default `GITHUB_TOKEN` | read-only, can't approve PRs | least privilege; the release job asks for `contents: write` itself |
| Allowed actions | GitHub-owned + `Swatinem/rust-cache@*` | a PR can't pull in arbitrary third-party actions |
| Merging | squash only, PR title/body as the commit, delete branch on merge, auto-merge allowed | one commit per PR on main |
| Security | secret scanning + push protection, private vulnerability reporting (were on), Dependabot alerts + security updates (turned on) | |

Public repositories get unlimited standard GitHub-hosted runner minutes, even on a free account. The CI limits are
about abuse and noise, not cost: PR runs cancel when superseded (`concurrency`), every job has `timeout-minutes`,
and Dependabot groups its version updates into one PR per ecosystem and week (at most 3 open; security updates
come as they're found).

## Release flow

`main` can't be pushed to, so a release takes two steps:

1. `just bump X.Y.Z`: branch `release-vX.Y.Z`, `scripts/bump.sh` (sets `Cargo.toml`/`Cargo.lock`, turns
   `[Unreleased]` into `[X.Y.Z] - date` with its compare link, refuses when `[Unreleased]` is empty), commit, push, PR.
2. After the PR is merged: `just release X.Y.Z` checks that main is at that version and has the changelog
   section, then creates the signed tag `vX.Y.Z` on the commit that set the version (PRs merged after it wait
   for the next release), pushes it and watches that tag's release run.

`release.yml` checks the tag against `Cargo.toml` and `CHANGELOG.md`, runs CI, uses the changelog section as the
release notes, and creates the release as a draft, attaches all assets, then publishes. A published immutable
release can't take more assets.

## Changelog and versions

`CHANGELOG.md` follows Keep a Changelog 1.1.0, and versions follow semver. 1.0.0 is the first public release:
the spec format, CLI and exit codes are the public API from here on. User-visible changes get an `[Unreleased]`
line in the same PR.

The `v0.1.0` tag from the withdrawn test release still exists. The tag ruleset now protects it like any other
`v*` tag.
