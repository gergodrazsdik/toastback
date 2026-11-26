# 0017: notify-send: a subcommand, a wrapper installed by default, and an opt-in wait

- Status: accepted
- Date: 2026-11-26

## Context and problem statement

Scripts and programs in WSL often report through `notify-send` (libnotify), which needs a Linux notification daemon
that WSL doesn't have. toastback shows Windows toasts from WSL, but with its own command line (`-t`, `-m`, a JSON
spec), so each script has to be changed to use it. The goal: scripts that call `notify-send` show toasts unchanged.

notify-send 0.8 (`notify-send [OPTION…] SUMMARY [BODY]`) differs from toastback's `show`:

- It prints nothing unless asked (`-p`), and every error exits 1. `show` prints the id and has exit codes 2, 3 and 4.
- Its options follow GLib's rules (options after the positionals, `-tVALUE`, clustered flags, a value is taken
  even when it starts with `-`), and `-t`, `-h` and `-v` mean other things than in `show`.
- The body takes backslash escapes (`g_strcompress`) and, with most daemons, simple markup.
- Ids are numbers that scripts pass back with `-r` to replace a notification. Hints such as `x-dunst-stack-tag`
  replace by tag.
- `-w` and `-A` block until the notification is closed or an action is chosen and print the action's name. The plan
  makes toastback asynchronous and lists "wait mode" as out of scope ([0001](0001-baseline.md)).

## Considered options

1. Where the translation lives:
   1. a POSIX `sh` wrapper that turns notify-send's arguments into toastback flags or a JSON spec;
   2. a third exe built from the same library (`notify-send.exe`);
   3. a subcommand of the existing exe, `toastback notify-send`, plus a small wrapper named `notify-send`.
2. The wrapper: opt-in (`install --notify-send`), or written by `install` by default.
3. `critical`: the `urgent` scenario (Windows 11: breaks through Do not disturb) or `reminder` (stays on screen).
4. `--version`: toastback's version, or a libnotify version that scripts can compare.
5. `-w` and `-A` before toastback can wait: degrade (show without buttons, print nothing), or exit 1.
6. Waiting at all: keep toastback strictly asynchronous, or add an opt-in wait.

## Decision outcome

Chosen:

- **A subcommand, `toastback notify-send`** (option 1.3). Its parser (`src/notify_send.rs`) follows GLib's rules and
  notify-send's messages, and is unit-tested on the Linux host. The arguments after `notify-send` reach it untouched:
  clap and the `-t`/`-m` dash-value handling never see them. It prints only what notify-send prints, and every error
  exits 1. A `sh` wrapper would have to escape JSON and strip markup in POSIX `sh`, and a third exe would still need
  a WSL shim for the caller's context, plus its own release assets, winget entry and attestation.
- **`install` writes `~/.local/bin/notify-send` by default** (option 2), next to the shim and in the same `wsl.exe`
  call. It runs `$HOME/.local/bin/toastback notify-send "$@"`. Its second line carries the marker
  `# toastback notify-send wrapper`, and `install` writes it only when no file is there or the file there carries the
  marker: anything else (a file, a link) is left alone with a note. `install --no-notify-send` removes ours and is
  recorded (`no_notify_send` in `install.json`), so later installs and upgrades keep it off; `install --notify-send`
  turns it back on. `uninstall` and `install --wsl none` remove it with the shim, by marker. `doctor` reports
  whether it's there, current, ours, and first on the login shell's PATH. An alias isn't enough: scripts don't read
  the interactive shell's aliases, so the wrapper is a file on PATH, ahead of `/usr/bin`.
- **`critical` is the `urgent` scenario** (option 3), with a **Dismiss** button because a scenario needs one. `low`
  is a silent toast.
- **`--version` prints `notify-send 0.8.9 (toastback X.Y.Z)`** (option 4): the libnotify version whose command line
  this follows, so scripts that check the version before using an option compare against libnotify.
- **`-w`, `-A`, `--id-fd`, `--selected-action-fd` and `--activation-token-fd` exit 1** with "isn't supported by
  toastback yet" (option 5), until toastback can wait. A script that asks for a choice gets an error, not a silent
  "dismissed".
- **An opt-in wait is approved** (option 6): `toastback --wait` (print the clicked button's id) and
  `toastback wait ID`, then notify-send's `-w` and `-A` on top of them. Asynchronous stays the default; nothing waits
  unless asked. This amends [0001](0001-baseline.md)'s "wait mode" exclusion. Its design (how the waiting process
  learns of the click, reporting a dismissed toast, exit codes, Ctrl-C through WSL interop) is decided with its
  implementation.

The mapping:

| notify-send | toastback |
|---|---|
| `SUMMARY [BODY]` | `title`, `body`; the body's escapes are applied, its markup tags removed and its entities decoded. An empty summary takes the body's place |
| `-u low`, `normal`, `critical` | `sound: silent`; default; `scenario: urgent` with a Dismiss button. Hint `urgency` (a byte) the same |
| `-i`, `-n`, hint `image-path` | `icon`, first usable of: the hint, `-i`, `-n`. A path or `file://` URI, or `dialog-information`/`-warning`/`-error`/`-question` (also `-symbolic`) as the built-in icons. Other icon names, SVG/XPM files and files that can't be read are left out, never an error |
| `-a` | `attribution` |
| `-e`, hint `transient` | `expires: 10s` |
| `-p`, `-r N` | print `N`; id `ns-N`. Without `-r`, a random `N` from 1 to 2³¹−1 |
| hints `x-canonical-private-synchronous`, `x-dunst-stack-tag`, `synchronous`, `private-synchronous` | `N` from the tag (FNV-1a), so the same tag replaces the toast; `-r` wins |
| hint `suppress-sound` | `sound: silent` |
| `-t`, `-c`, other hints | checked like notify-send checks them, then ignored |

### Consequences

- Once installed, `notify-send` in that distro shows Windows toasts instead of failing for lack of a daemon. To keep
  the distro's own: `install --no-notify-send`, or delete the file and install with that flag.
- Not covered: systemd services (their PATH has no `~/.local/bin`), programs that run `/usr/bin/notify-send` by full
  path, and programs that send notifications over D-Bus themselves (libnotify bindings, GUI apps under WSLg). The
  last would need a Linux-side `org.freedesktop.Notifications` service, which doesn't fit a single Windows exe.
- notify-send's notifications have ids `ns-<number>`, for `toastback log` and `toastback remove`.
- `toastback notify-send` is a command now, so it no longer means "show the spec file named `notify-send`", as for
  every other command name. Additive otherwise: a minor release.
- `-t` has no effect: Windows decides how long a toast stays on screen (GNOME Shell ignores it too). A `duration`
  field and progress bars (hint `int:value`) are possible later additions.

## More information

libnotify's `tools/notify-send.c` (0.8.9) was the reference for the option table, the body escapes, the hint syntax
and the messages. Its `--action` help says unnamed actions are numbered from 0; the code numbers them from 1, which
`-A` should follow when it's implemented.
