# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `open` targets and `exe` programs accept WSL paths: `/mnt/c/...`, `/home/...`, `./x` and `../x`.

### Changed

- A clicked notification's `env` values are removed from its pending file at once, instead of staying until it expires.
- `install --wsl NAME` refuses a distro that doesn't exist, and `install --wsl ...` exits with an error when a shim it
  was asked for can't be written (it used to warn and exit 0).
- `install --wsl none` removes the shims of earlier installs.

### Fixed

- An image that can't be deleted (in use) no longer stops `prune`, `remove` or `clear` from deleting the other images,
  and `prune` tries it again later.
- A toast that fails to show no longer leaves its pending file and cached images behind.
- `show` exits 3, not 2, when it can't write to the image cache.
- `validate` reads `icon` and `image`, so one that `show` couldn't read no longer passes.
- `clear` no longer counts files it leaves alone, and names them.
- `install` could wait forever for a program started in WSL that kept `wsl.exe`'s output open, despite the time limits.
- `install` could run a `wsl.exe` planted next to the downloaded exe. It now runs `wsl.exe` and `cmd.exe` from System32.
- A first install that couldn't write a shim (no WSL yet, Docker Desktop's distro as the default) no longer counts as
  `--wsl none`: later installs and `winget upgrade` write the shim to the default distro.
- A later install no longer writes shims again after `--wsl none`.
- A failed or interrupted upgrade can no longer leave a missing or partly written exe.
- `install --add-to-path` no longer claims a PATH entry you added yourself, so `uninstall` leaves it alone.
- A damaged `install.json` no longer blocks `install` and `uninstall`, and `show` reports it instead of "not installed".
- `install` warns when it can't update `toastback-cli.exe`, reports a failed PATH check as such, and shows `wsl.exe`'s
  own error when the distro can't start.
- Output of localized `wsl.exe` messages could come out garbled in failure toasts and `toastback log`.
- A relative `icon`, `image` or `cwd` with a Windows `context.cwd` no longer builds a broken path, and one with a
  `context.cwd` like `~/x` is an error at show time.
- A NUL in a callback's command, `env`, `cwd`, `distro` or `user` is refused when showing, instead of failing on the
  click; one in the title no longer stops the callback.
- A `WSLENV` in a callback's `env` is merged with toastback's instead of replaced, and keeps its flags.
- `&` in a button label shows as `&` in dialogs too.
- U+FFFE and U+FFFF in notification text no longer make `show` fail.
- Ids that differ only in case (`Deploy`, `deploy`) are one notification: the second toast replaces the first, instead
  of leaving a toast with dead buttons that `remove` couldn't reach.
- `--help` starts with what toastback does, not with the description of its WSL options.
- Text for `-t`, `-m` and `-d` that matches a flag of another command (`-m --json`, `-m -n`) is taken as text instead of
  failing the notification.

## [1.3.0] - 2026-10-31

### Added

- winget package `GergoDrazsdik.Toastback` (from this version on), which installs, upgrades and uninstalls toastback
  including its WSL shim.
- `install --wsl default` writes the shim to the default WSL distro only.
- `install --quiet` skips the test notification.
- An Apps & Features entry, so toastback can be uninstalled from Windows' app list; `doctor` checks it.

### Changed

- From Windows, `install` without `--wsl` now writes the shim to the default distro on a first install, instead of to
  every distro, and to the distros of the last install on a later one.
- `install --wsl all` skips Docker Desktop's distros (`docker-desktop*`). Name one with `--wsl` to include it.
- `install` gives up on a WSL command after a time limit instead of waiting forever for a stuck distro.
- The exes' version info names Gergő Drazsdik as the company and copyright holder.
- `just install` also adds toastback to the Windows PATH.

## [1.2.0] - 2026-10-23

### Added

- Built-in icons: `icon` (and `-i`) accepts `info`, `warning`, `error` or `question` instead of an image path.
  Toasts show a bundled PNG as the logo; dialogs show the matching Windows icon, which also brings the error and
  question icons to dialogs.
- `toastback examples [NAME]` lists built-in example specs or prints one, so they're available without the
  repository: `simple`, `warning`, `build`, `deploy`, `export`, `reboot-dialog`, `alarm`. `--help` shows a few
  one-line examples.

### Changed

- The examples read as real use cases: `deploy` runs an inline command instead of `examples/deploy.sh` (removed),
  `reboot-dialog` asks with the question icon and re-shows itself on "Remind me later", and `alarm` snoozes with
  `toastback examples alarm`, so it works outside the repository.

## [1.1.0] - 2026-10-14

### Changed

- The README installs with download, check, review, run (`install.sh` plus `SHA256SUMS`) instead of piping
  `install.sh` into bash. Releases also attest `install.sh`.
- README: warn that `cmd` callbacks must not use `%TOASTBACK_TITLE%`, `%TOASTBACK_DATA%` or other `%VAR%`s holding
  untrusted text (cmd expands them before parsing `&`, `|`, …), with the safe patterns for `wsl` and `pwsh`.

- The default `expires` for toasts is 3 days, the most Windows keeps a toast in the notification center (dialogs
  keep 7 days); a longer `expires` keeps a toast's buttons for 3 days and still applies to dialogs. `expires` is
  limited to 365 days.
- A `distro` other than the caller's (in a run or in the spec's `context`) runs as that distro's default user in
  its home directory (it used to take the caller's user and directory), unless `user`/`cwd` are set. A caller
  whose distro isn't known keeps its user and directory.
- `install` without a working `wsl.exe` (from Windows, default `--wsl`) installs anyway and warns that no shim was
  written; `--wsl all` fails before changing anything.
- `install --wsl all` gives the calling distro's shim to the calling user, like the default does.
- `open` expands `%VAR%`s in paths only, no longer in URLs (`?q=%OS%` stays as it is). The README said it
  expanded `%VAR%`s in general; `TOASTBACK_*` were never expanded, and stay so.
- Notification ids can't start with `-` or be a Windows device name (`CON`, `NUL`, `COM1`, …). Notifications
  that 1.0.0 showed with a leading `-` still work.

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
- A choice made in a dialog that stayed open past `expires` was dropped: one in a dialog that opened before
  `expires` now counts for up to a day after it.
- A dialog that a callback opens (`toastback --mode dialog` inside a `pwsh` callback) is no longer closed when the
  callback times out.
- `cmd` callbacks (and `exe` callbacks that run a `.bat` or `.cmd` script) with a WSL `cwd` ran in `C:\Windows`
  (cmd.exe refuses UNC directories): such specs are rejected at show time. A relative `cwd` is resolved against the
  caller's directory for `wsl` callbacks too, and from cmd or PowerShell against the current directory for
  Windows-side callbacks. Errors that would only show at click time (a relative `cwd` without a caller directory,
  `~` in a Windows-side `cwd`) now fail `toastback` itself.
- Spec files written by Windows PowerShell (UTF-8 with BOM, UTF-16) are read.
- `-t`, `-m` and `-d` accept text that starts with `-` (like cmake's `-- Build files have been written`).
- A piped spec is read also with `--mode`, `--id`, `-i`, `-d` or `--scenario`; only `-t`/`-m` replace it.
- Removing a notification no longer deletes the cached images of another one whose id starts with the same text
  and a `-` (`build` and `build-2`).
- Two processes rotating the log at once could make a click fail or lose the rotated log; concurrent log lines
  could interleave.
- Pruning deleted pending notifications whose file it couldn't fully parse, such as ones written by a newer
  version.
- `toastback log | head`, `toastback install 2>&1 | head` (any closed stdout or stderr) aborted with a panic,
  `install` halfway through. Other output errors (a full disk) now make the exit code 1, except for `show`, whose
  exit code still only says whether the notification was shown.
- wsl.exe's own error messages (UTF-16) are decoded, so they reach the log and the "failed" toast readable, also
  in other languages and next to the callback's own output.
- `remove` stopped with an error at a pending file written by a newer version.
- A click or `remove` for an id that Windows reserves as a device name (`NUL`) is ignored rather than touching
  the device.
- Distro names are compared case-insensitively by `install` and `uninstall`.
- `install` checks where the shim goes before it changes anything: `--wsl all` without a working `wsl.exe` no
  longer leaves registry keys and a PATH entry behind without an install record.
- `install.sh`: `--wsl`, `--distro` or `--user` among its arguments no longer clash with its own; the Windows
  temp dir is
  found also when the profile path has non-ASCII characters or `&`; the downloaded exes are made executable; it
  runs on bash before 4.4 too; a failing `toastback.exe install` is no longer always blamed on Windows App Control.
- The shim's "Regenerate" comment suggested a command that empties the shim; it now says `toastback install`.
- README install steps: the comments after the commands broke them in zsh, the checksum now gates reading the
  script, and the downloads go to a fresh directory.
- `just release` tags the commit that set the version (not main's head, and it asks again when that's another
  commit), watches the run for that tag and fails when it fails; `just bump` checks everything before creating the
  branch, and `scripts/bump.sh` before editing any file, without needing `cargo`.
- Release workflow: a re-run after a failed publish reuses the draft (and an API error no longer creates a
  second one); the oldest version's notes no longer end with the changelog's link definitions, and a reference
  link inside a section doesn't cut it short.
- README: a program a callback leaves running in the background should redirect its output.

### Security

- Callback environment values (`env`, which may hold tokens) are no longer written to the event log: the
  `callback_started` event lists the variable names only. Logs written by 1.0.0 still hold them: delete
  `%LOCALAPPDATA%\toastback\log` to remove them.

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

[unreleased]: https://github.com/gergodrazsdik/toastback/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/gergodrazsdik/toastback/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/gergodrazsdik/toastback/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/gergodrazsdik/toastback/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/gergodrazsdik/toastback/releases/tag/v1.0.0
