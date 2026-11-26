# 0015: Pinned CI: actions by commit SHA, linters by version and digest, a 7-day cooldown

- Status: accepted
- Date: 2026-11-26

## Context and problem statement

The workflows referenced actions by moving tags (`actions/checkout@v7`), installed yamllint unpinned and built
actionlint from source, and the release workflow holds `contents: write` and attestation permissions. A moved tag
or a compromised release of any of them would run with those permissions. The local linters (`just lint`,
lefthook) also had no CI counterpart beyond shellcheck, yamllint and actionlint.

## Considered options

1. Keep tags, and rely on Dependabot and the allowed-actions setting.
2. Pin every action to a full commit SHA (pinact) and audit the workflows (zizmor); install the linters CI needs
   at exact versions, with SHA-256 digests for release binaries; take new releases only once they are 7 days old.

## Decision outcome

Chosen option: 2, the same setup as the maintainer's other repositories.

- **Actions** are pinned to full commit SHAs with the version as a comment (`just pin`); `just lint` checks it
  (`pinact run --check`). Dependabot keeps them current, with a 7-day cooldown.
- **A `lint` workflow** runs `just lint` with `TOASTBACK_REQUIRE_TOOLS=1`, so a missing tool fails instead of being
  skipped. Release binaries come from `scripts/install-ci-tools.sh`, each checked against its pinned digest;
  yamllint and zizmor come from PyPI and markdownlint-cli2 from npm at exact versions set in `lint.yml`.
- **zizmor** audits the workflows: offline in `just lint` and the hooks, online (known-vulnerable actions, impostor
  commits) in the `lint` workflow's `zizmor` job, which uploads SARIF to code scanning with GitHub's own
  `codeql-action`. No third-party action is added, so the allowed-actions setting stays as it is
  ([0012](0012-repository-and-release.md)).
- **Every `actions/checkout`** sets `persist-credentials: false`; the release job uses `GH_TOKEN` for `gh`, not git
  credentials.
- **Pins outside Dependabot's reach** (the linters, and cargo-xwin in `ci.yml`): `just tool-updates` reports newer
  releases and when each is due. Updating one is a pull request that changes the version, and for release binaries
  the URL and digest, together.

### Consequences

- `ci.yml` no longer runs the linters; its `build` job keeps rustfmt and clippy.
- The release workflow's call to `ci.yml` keeps the `./` form: zizmor suggests a newer self-repository syntax that
  actionlint 1.7.12 rejects, so that finding is ignored inline.
- `lint` and `zizmor` become required checks once they have run on `main`.
