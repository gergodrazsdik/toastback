# 0008: Commands and click feedback: `validate`, `remove`, `clear`, `doctor`, `notify_on_success`

- Status: accepted
- Date: 2026-10-14

## Context and problem statement

M3 rounded off the CLI: checking specs without showing them, taking a notification back, checking an installation,
and telling the user how a clicked button's command went. The plan named the commands; their exact behaviour and exit
codes are part of the public API since 1.0.0.

## Considered options

1. Feedback after a click: only on failure (`notify_on_error`, the plan), or also on success, opt-in.
2. `doctor`'s notification check before the first toast: a failure, or a warning ([0002](0002-platform-spikes.md)).

## Decision outcome

Chosen:

- **`notify_on_success`**, a new spec field, default `false`. On success it shows "<button label>: done" with the
  notification title, the duration and the last line of stdout. `notify_on_error` (default `true`) shows "<label>:
  failed" with the exit code, timeout or start error and the last line of stderr (or stdout). Feedback toasts have no
  buttons and no body action, and never ask for feedback themselves. `activate::feedback` builds them; it's pure and
  tested on the host.
- **`validate FILE...`**: the same loading as `show` (context, flags, validation), plus a check that `icon` and `image`
  can be read. Exit 2 if any file fails. `just test` runs it on `examples/*.json` through the Windows exe.
- **`remove ID...`**: removes the toast from Action Center and deletes the pending file, so its buttons stop working.
  A dialog already open for that id runs nothing when answered: `finish` claims the click first, and the claim finds
  the pending file gone. Exit 1 if an id is unknown. **`clear`** does the same for all.
- **`doctor [--json]`** checks the install record, the exe, the app id, the protocol command (it must point at the
  installed exe), the notification setting, the user PATH (if `--add-to-path` was used), `wsl.exe`, the calling
  distro's shim (byte-compared with what `install` would write) and that the state folder is writable. Exit 1 on any
  failure; warnings don't count. The notification setting's error before the first toast is a warning.

### Consequences

- `log`, pruning pending files on every run, the callback `timeout` and `--wsl all` were pulled forward from M3 into
  M1, because M1's acceptance needed them.
- The registry code moved to `registry.rs` (`get_string`, `set_string`, `delete_tree`, `key_exists`).
