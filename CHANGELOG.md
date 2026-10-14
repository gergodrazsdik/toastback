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

- The default `expires` is 3 days, the most Windows keeps a toast in the notification center; a longer `expires`
  keeps a toast's buttons for 3 days and still applies to dialogs. `expires` is limited to 365 days.
- A `distro` other than the caller's runs as that distro's default user in its home directory (it used to take
  the caller's user and directory), unless the run sets `user`/`cwd`.
- `open` expands `%VAR%`s in paths only, no longer in URLs (`?q=%OS%` stays as it is). The README said it
  expanded `%VAR%`s in general; `TOASTBACK_*` were never expanded, and stay so.
- Notification ids can't start with `-` or be a Windows device name (`CON`, `NUL`, `COM1`, …).

### Fixed

- The WSL shim found no distro name when `WSL_DISTRO_NAME` was set but empty: `wslpath` fails then, so the
  fallback now runs it without the variable. Re-run `install` to update the shim.
- README: a repeated sentence under Limitations.
- A callback's timeout now ends everything the callback started (a job object), not only cmd.exe or pwsh.exe, and
  a program it leaves running (`start notepad`) no longer keeps the click handler waiting for its output.
- Two clicks on one notification (or a click and its dialog) could both run the callback; the first now claims
  it before anything runs.
- A notification shown again with the same id while the previous one's callback ran was overwritten when that
  callback ended, and its new toast removed. `remove`/`clear` during a callback were undone the same way.
- A choice made in a dialog that stayed open past `expires` was dropped: an open dialog now keeps its
  notification for up to a day longer.
- `cmd` callbacks with a WSL `cwd` ran in `C:\Windows` (cmd.exe refuses UNC directories): such specs are rejected
  at show time. A relative `cwd` is resolved against the caller's directory for `wsl` callbacks too, and errors
  that would only show at click time (a relative `cwd` without a caller directory) now fail `toastback` itself.
- Spec files written by Windows PowerShell (UTF-8 with BOM, UTF-16) are read.
- `-t`, `-m` and `-d` accept text that starts with `-` (like cmake's `-- Build files have been written`).
- A piped spec is read also with `--mode`, `--id`, `-i`, `-d` or `--scenario`; only `-t`/`-m` replace it.
- Removing a notification no longer deletes the cached images of another one whose id starts with the same text
  and a `-` (`build` and `build-2`).
- Two processes rotating the log at once could make a click fail; concurrent log lines could interleave.
- Pruning deleted pending notifications whose file it couldn't fully parse, such as ones written by a newer
  version.
- `toastback log | head` (or any closed stdout) aborted with a panic.
- WSL callbacks get `WSL_UTF8=1`, so wsl.exe's own errors reach the log and the "failed" toast readable.
- Distro names are compared case-insensitively by `install` and `uninstall`.
- `install` checks where the shim goes before it changes anything: `--wsl all` without a working `wsl.exe` no
  longer leaves registry keys and a PATH entry behind without an install record.
- `install.sh`: `--wsl …` among its arguments no longer clashes with its own `--wsl`; the Windows temp dir is
  found also when the profile path has non-ASCII characters or `&`; the downloaded exes are made executable; a
  failing `toastback.exe install` is no longer reported as blocked by Windows App Control.
- The shim's "Regenerate" comment suggested a command that empties the shim; it now says `toastback install`.
- README install steps: the comments after the commands broke them in zsh, the checksum now gates reading the
  script, and the downloads go to a fresh directory.
- `just release` tags the commit that set the version (not main's head), watches the run for that tag and fails
  when it fails; `just bump` checks everything before creating the branch, and `scripts/bump.sh` before editing
  any file, without needing `cargo`.
- Release workflow: a re-run after a failed publish reuses the draft; the oldest version's notes no longer end
  with the changelog's link definitions.

### Security

- Callback environment values (`env`, which may hold tokens) are no longer written to the event log: the
  `callback_started` event lists the variable names only.

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
