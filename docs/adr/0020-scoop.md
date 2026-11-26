# 0020: Scoop: a bucket of our own, with the same nested install as winget

- Status: accepted
- Date: 2026-11-26

## Context and problem statement

Scoop was out of scope for v1 ([0001](0001-baseline.md)), and this record takes it in: Scoop users install their
command-line tools with it. A Scoop manifest describes a download, a hash, commands to put on PATH and PowerShell
hooks, and lives in a bucket, a git repository of manifests. toastback installs itself (`toastback install`), and
the package has to keep that working through `scoop install`, `scoop update` and `scoop uninstall`, as
[0013](0013-winget.md) did for winget.

Two things about Scoop shape the package. `scoop update` runs the old version's `pre_uninstall` hook and
uninstaller before it installs the new version, so an uninstall hook that ran `toastback uninstall` would wipe the
install record on every update, and with it the `--wsl` and `--no-notify-send` choices. And Scoop skips an update
while a program runs from the app's folder.

## Considered options

1. The bucket: Scoop's `main` (at least 500 stars and 150 forks, a standard install), `extras` (well-known apps), a
   bucket in this repository, or a bucket repository of our own.
2. The install: the nested install winget uses (Scoop unpacks the release zip and runs `toastback-cli install`), or
   registering the exe where Scoop put it, which needs a new `install --in-place` option.

## Decision outcome

Chosen, with the maintainer's approval:

- **A bucket repository of our own**, `gergodrazsdik/scoop-bucket`, made from Scoop's BucketTemplate. Its bot
  (Excavator) checks for new releases every 4 hours and updates the manifest through `checkver` and `autoupdate`,
  taking the hash from the release's `SHA256SUMS`. A bucket in this repository would need a pull request after every
  release, since the zip's hash only exists after the release build. `main` and `extras` can come later, once
  toastback is known well enough.
- **The nested install** (option 2.1). The installer script runs `toastback-cli.exe install --quiet` from Scoop's
  folder and fails the install on a non-zero exit code. toastback then lives in `%LOCALAPPDATA%\toastback`, like
  with every other method, so clicks, callbacks and dialogs run from there and never block a `scoop update`.
  `bin` puts Scoop's own `toastback-cli` shim on PATH, so there's no `--add-to-path`.
- **Uninstall only on `scoop uninstall`.** Scoop runs commands as `exec($cmd, …)`, so hooks see `$cmd`:
  `pre_uninstall` runs `toastback-cli uninstall` only when `$cmd` is `uninstall`, with `--purge` when Scoop's
  `--purge` was given (`$purge`). `scoop update` then simply installs over the old version, as `winget upgrade` does.

The template is `packaging/scoop/toastback.json`. `just scoop-manifest X.Y.Z` renders a release's manifest with the
hash from its `SHA256SUMS` and checks it against Scoop's JSON schema; `just scoop-test` serves the local build over
`http://localhost`, installs it through Scoop, updates it with a probe toast showing (an update that uninstalled would
clear it) and uninstalls it, then runs `just install`.

### Consequences

- `scoop update toastback` refreshes the WSL shims and the notify-send wrapper, like every re-install.
- Installing through Scoop and through winget on the same machine manages one installation twice; the README says to
  pick one.
- The bucket repository is published once, by hand, with the first manifest from `just scoop-manifest`; after that
  its bot keeps it current.

## More information

Scoop's hooks and the `$cmd` variable were read from its source (`libexec/scoop-update.ps1`,
`libexec/scoop-uninstall.ps1`, `lib/commands.ps1`). Tested live with Scoop 0.6.0: install, an update that kept the
probe toast, and an uninstall that left no files, registry keys or shims behind.
