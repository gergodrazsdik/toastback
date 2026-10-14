# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The README installs with download, check, review, run (`install.sh` plus `SHA256SUMS`) instead of piping
  `install.sh` into bash. Releases also attest `install.sh`.
- README: warn that `cmd` callbacks must not use `%TOASTBACK_TITLE%`, `%TOASTBACK_DATA%` or other `%VAR%`s holding
  untrusted text (cmd expands them before parsing `&`, `|`, …), with the safe patterns for `wsl` and `pwsh`.

### Fixed

- The WSL shim found no distro name when `WSL_DISTRO_NAME` was set but empty: `wslpath` fails then, so the
  fallback now runs it without the variable. Re-run `install` to update the shim.
- README: a repeated sentence under Limitations.

## [1.0.0] - 2026-10-14

First public release. (A v0.1.0 test release was published and withdrawn; its `install.sh` didn't work.)

### Added

- `toastback` shows a Windows toast notification or a native dialog and returns at once, printing the
  notification id. Exit codes: 0 shown, 2 invalid spec, 3 failed to show, 4 not installed.
- A JSON spec (from a file, stdin or quick flags `-t -m -d -i --mode --scenario --id`): title, body, details,
  icon, image, attribution, scenario (`default`, `reminder`, `alarm`, `urgent`), sound, expiry, body click
  behaviour, up to 5 buttons with optional styles, a `default` handler, arbitrary `data`, and the caller's context.
- Button callbacks run when clicked, even long after `toastback` exited: in WSL (`bash -lc` string or argv, in the
  calling distro, user and directory by default), `cmd`, PowerShell, any exe, or `open` (URL, file, folder), with
  `cwd`, `env`, `distro`, `user` and `timeout` options. Callbacks get `TOASTBACK_ID`, `TOASTBACK_ACTION`,
  `TOASTBACK_TITLE` and `TOASTBACK_DATA`.
- Dialog mode (`mode: dialog`) and a details dialog when a toast's body is clicked; dialogs stay on top.
- Feedback toasts: "failed" when a callback fails (`notify_on_error`, on by default) and "done" when it succeeds
  (`notify_on_success`).
- Commands: `install` (`--wsl`, `--add-to-path`), `uninstall` (`--purge`), `doctor` (`--json`), `validate`, `log`,
  `remove`, `clear`, `shim`.
- `toastback-cli.exe`: the same program as a console app, for interactive cmd and PowerShell.
- A WSL shim (`~/.local/bin/toastback`) that passes the caller's distro, user and directory, also from systemd
  services and cron.
- `install.sh`, a one-line installer for WSL that verifies the downloads against `SHA256SUMS`.
- Release assets with GitHub build provenance attestations.

### Security

- Clicks are accepted only with the notification's random 128-bit token; commands never come from the activation
  URI, and notification data reaches callbacks as environment variables, never pasted into a command line.

[unreleased]: https://github.com/gergodrazsdik/toastback/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/gergodrazsdik/toastback/releases/tag/v1.0.0
