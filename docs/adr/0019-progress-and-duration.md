# 0019: Progress bars, `toastback update`, and the banner's duration

- Status: accepted
- Date: 2026-11-26

## Context and problem statement

notify-send scripts send a percentage with the hint `int:value:N` (volume, brightness, downloads), and ask for a
longer banner with `-t`. Windows toasts have a progress bar and a `long` duration, but toastback's spec had
neither. A progress bar that only changes by showing the toast again pops a new banner, with its sound, at every
step. Both are additions to the spec and the CLI, a public API.

## Considered options

1. The spec: a nested `progress` object with the bar's four parts, or flat `progress` and `progress_status` fields.
2. Changing a bar: a `toastback update ID` command that updates the toast in place, or only showing it again under
   the same id.
3. The duration: a spec field and a flag, or a notify-send-only mapping.

## Decision outcome

Chosen, with the maintainer's approval:

- **`"progress": {"value", "status", "title", "label"}`** (option 1): `value` is `0`–`1` or `"indeterminate"`, the
  others are optional text (`label` replaces the percentage). Toasts only: a dialog with `progress` is invalid. On
  the command line, `--progress` takes `0.4`, `40%` or `indeterminate`, and `--progress-status` the status line.
- **`toastback update ID`** (option 2) changes the bar through Windows' `ToastNotifier.Update`: no new banner, no
  sound. The toast's `<progress>` element is bound to data (`{progressValue}`, `{progressStatus}`, and
  `{progressTitle}`/`{progressValueString}` when the toast has a title/label), and `show` attaches the first values.
  So `update` can change what the toast was shown with, and refuses a title or label it didn't have. Windows wants a
  status, so an empty one is sent when there's none. Updates use sequence number 0 (always apply). The new progress
  is saved in the pending file too. Exit 0 updated, 2 nothing to update or not possible (a dialog, no bar), 3
  Windows failed, 5 no longer shown (answered, dismissed, removed, expired; also an unknown id, as `wait` treats a
  removed one).
- **`"duration": "short" | "long"` and `--duration`** (option 3): Windows' toast attribute. `short` follows the
  user's notification setting (5 s by default), `long` is about 25 seconds.
- **notify-send**: `int:value:N` is a progress bar at N %, clamped to 0–100 as the daemons do. With a stack tag or
  `-r`, each new value replaces the toast (a new banner, as the daemons show a new popup); in-place updates are for
  `toastback update`. `-t 0` (libnotify's "never expires") and `-t` over 10 000 ms make a long banner.

### Consequences

- A progress toast costs nothing extra until it's updated: the data travels with `Show`.
- `update` after the toast left the notification center gets Windows' "not found", and exits 5.
- Changing anything else about a toast (title, body, buttons) still means showing it again under its id.

## More information

Tested live: a bound bar shows with a value, a status, and a title and label; `update` moves it to 50 %, to
indeterminate and to a new label; it refuses a title the toast wasn't shown with (exit 2), a dialog with a progress
bar is invalid (exit 2), and an unknown id or a toast taken out of the notification center exits 5.
