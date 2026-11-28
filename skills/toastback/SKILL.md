---
name: toastback
description: >-
  Show Windows toast notifications and dialogs from WSL or Windows scripts with toastback, with buttons that
  run commands when clicked. Use when a script, job or agent should tell the user something, ask them a
  question and wait for the answer, show the progress of a long job, or when writing or fixing a toastback
  JSON spec.
---

<!-- toastback skill for toastback {{VERSION}}. `toastback install --skill` keeps this file current. -->

# Using toastback

toastback shows a Windows toast or a native dialog and returns at once, printing the notification's id. When the
user clicks a button later, Windows starts toastback again and it runs the command attached to that button, in WSL
or on Windows. It works from WSL (`toastback`, a shim in `~/.local/bin`), from PowerShell or cmd (`toastback-cli`)
and from Task Scheduler.

## Pick the right form

| You want | Use |
|---|---|
| Tell the user something | `toastback -t "Build done" -m "Release build finished in 4 min"` |
| A built-in icon | `toastback -t "Build failed" -i error` (`info`, `warning`, `error`, `question`) |
| Ask, and act on the answer in the script | `choice=$(toastback --wait --timeout 10m my.json)`: prints the clicked button's id |
| Ask, and act on the answer later (hours, after the script exits) | buttons with a `run` callback, no `--wait` |
| Ask about something already shown | `toastback wait "$id"` |
| Progress of a long job | `id=$(toastback -t Backup --progress 0)`, then `toastback update "$id" --progress 40% --status "Copying"` |
| A prompt that must be seen (Focus assist hides banners) | `"mode": "dialog"`, or `"scenario": "alarm"` with a button |
| A tool that already calls `notify-send` | nothing: `~/.local/bin/notify-send` is toastback's wrapper (see below) |
| Start from a working spec | `toastback examples` lists them, `toastback examples deploy > deploy.json` saves one |

A spec comes from a file, from stdin (`-`, or when neither `-t` nor `-m` is given) or only from flags; flags
override the spec's fields. Check a spec without showing it: `toastback validate my.json`.

## The spec

```json
{
  "id": "deploy-1234",
  "title": "Deploy to production?",
  "body": "Build 1234 passed all checks.",
  "details": "Commit abc123: fix the login form.",
  "icon": "question",
  "attribution": "CI",
  "scenario": "reminder",
  "buttons": [
    {
      "id": "deploy",
      "label": "Deploy",
      "style": "success",
      "run": { "wsl": "./scripts/deploy.sh prod", "timeout": "10m" }
    },
    { "id": "log", "label": "Open log", "run": { "open": "https://ci.example.com/build/1234" } },
    { "id": "later", "label": "Not now" }
  ],
  "data": { "build": 1234 },
  "notify_on_success": true
}
```

Fields (unknown fields are errors):

| Field | Meaning |
|---|---|
| `title` | required |
| `body`, `details` | text; `details` opens in a dialog when the toast's text is clicked |
| `id` | optional, generated if absent; showing again with the same id replaces the earlier notification |
| `mode` | `toast` (default) or `dialog` |
| `icon`, `image` | `info`, `warning`, `error`, `question`, or a PNG/JPG by WSL or Windows path (under about 200 KB) |
| `attribution` | a small grey line |
| `scenario` | `default`, `reminder`, `alarm` (both stay on screen and need a button), `urgent` |
| `sound` | `default`, `silent`, `alarm`, `reminder` |
| `duration` | banner time: `short` (Windows' setting) or `long` (about 25 s) |
| `progress` | toasts only: `{ "value": 0.4, "status": "…", "title": "…", "label": "…" }`; `value` is 0 to 1 or `"indeterminate"` |
| `expires` | how long it stays clickable: `90s`, `5m`, `1h30m`, `3d`. Default 3d for toasts (Windows' limit), 7d for dialogs |
| `on_click` | a click on the text: `details`, `default` or `none` |
| `buttons` | up to 5: `id`, `label`, optional `style` (`success`, `critical`) and `run` |
| `default` | the callback for buttons without `run`, and for a click on the text |
| `data` | any JSON, given to callbacks as `TOASTBACK_DATA` |
| `notify_on_error` | a "failed" toast when a callback fails (default true) |
| `notify_on_success` | a "done" toast when it succeeds (default false) |
| `context` | `distro`, `user`, `cwd` for WSL callbacks: the shim fills it in, so leave it out |

A callback (`run`, `default`) has exactly one of:

- `"wsl": "string"`: runs in `bash -lc` (`sh -lc` without bash), as you, in the directory you ran toastback from;
  `"wsl": ["argv", "…"]` runs the program directly, with no shell;
- `"cmd": "string"`, `"pwsh": "string"` (PowerShell 7, else Windows PowerShell);
- `"exe": ["C:\\path\\x.exe", "arg"]`, a Windows or WSL path;
- `"open": "url-or-path"`: the default app for a URL, file or folder.

It can also have `cwd`, `env` (an object of strings), `distro`, `user` and `timeout` (`"5m"`: killed after it).
Callbacks get `TOASTBACK_ID`, `TOASTBACK_ACTION` (the button id, or `__body`), `TOASTBACK_TITLE` and
`TOASTBACK_DATA` as environment variables.

## Waiting for an answer

`--wait` waits until the notification is answered and prints the answer instead of the id: the button's id, or
`__body` for a click on the text. The button's callback still runs. `--timeout` limits the wait.

```sh
choice=$(toastback examples reboot-dialog | toastback --wait --timeout 10m)
case $? in 0) echo "chose $choice" ;; 5) echo "dismissed" ;; 6) echo "no answer" ;; esac
```

`toastback wait ID [--timeout DURATION]` does the same for a notification that's already shown, also one answered
before the wait started.

## Progress bars

```sh
id=$(toastback -t "Backup" --progress 0 --progress-status "Starting")
toastback update "$id" --progress 40% --status "Copying photos"
toastback update "$id" --progress 1 --status "Done"
```

`toastback update` changes only what you pass, without a new banner or sound. `--title` and `--label` work only if
the toast was shown with a progress `title` or `label`. Exit 5 means it's no longer shown (clicked, dismissed or
expired): stop updating it.

## notify-send

`~/.local/bin/notify-send` runs `toastback notify-send`, which takes libnotify's command line: `notify-send "Title"
"Body"`, `-u critical`, `-i dialog-error`, `-p` (print the id), `-r ID` (replace), `-h int:value:40` (a progress bar),
`-A name=Label` (a button; prints the chosen name), `-w` (wait). Use it for tools that already call `notify-send`;
for new code, plain `toastback` gives buttons with callbacks, dialogs and specs. A notify-send wait exits 0 however
it ends, as notify-send does.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | shown (the id is on stdout); with `--wait` or `wait`: answered (the answer is on stdout); `update`: updated |
| 1 | another error, such as `remove` with an unknown id or a `notify-send` error |
| 2 | invalid spec, image or argument; `wait`: no notification with that id; `update`: nothing to update |
| 3 | Windows failed to show or update it |
| 4 | toastback isn't installed |
| 5 | `wait`: closed without a choice (dismissed, removed, replaced); `update`: no longer shown |
| 6 | `wait`: timed out, or the notification expired |
| 130 | `wait` stopped with Ctrl-C (from PowerShell or cmd; in WSL the shell reports 1) |

## Common mistakes

- **Expecting `toastback` to return the answer.** Without `--wait` it returns at once with the id. A dismissed or
  timed-out toast runs nothing: there is no dismiss callback, and `--wait` reports it as exit 5.
- **A button without `run`.** With no `default` either, it only closes the toast. That's fine with `--wait`; without
  it, the click does nothing you can see.
- **Bad ids.** Letters, digits, `.`, `_` and `-` only, at most 64, no leading `-`, and not a Windows device name
  (`CON`, `NUL`, `COM1`, …). Reusing an id replaces the earlier notification. Each notification takes one click.
- **Looking for callback output on screen.** There is none: read it with `toastback log` (stdout and stderr, last 8
  KB each), or set `notify_on_success`. `notify_on_error` is on by default, so failures show a toast.
- **Running toastback inside a sandbox that blocks Windows interop**, such as Claude Code's Bash sandbox. It can't
  start the Windows exe there: run the call outside the sandbox (an excluded command).
- **systemd services, timers and cron.** Call `~/.local/bin/toastback` by its full path: services don't get your
  shell's PATH. The distro is detected without `WSL_DISTRO_NAME`.
- **PowerShell and cmd.** Use `toastback-cli`: `toastback.exe` is a GUI program, so the shell doesn't wait for it or
  show its output.
- **Untrusted text in callbacks.** In `cmd`, never use `%TOASTBACK_TITLE%`, `%TOASTBACK_DATA%` or `env` values (cmd
  expands them before parsing `&` and `|`). In `wsl`, quote `"$TOASTBACK_TITLE"` and never `eval` it. Prefer the
  argv form, `pwsh` or `exe`.
- **`cmd` in a WSL directory.** cmd.exe refuses UNC paths, so a `cmd` callback needs a `cwd` under `/mnt/<drive>`;
  use `pwsh` otherwise.
- **Toasts from Task Scheduler or services in session 0.** Toasts need a logged-on user: "Run only when user is
  logged on".

## Commands

| Command | What it does |
|---|---|
| `toastback [FILE]`, `toastback show` | show a notification (the default command): `--title`/`-t`, `--body`/`-m`, `--details`/`-d`, `--icon`/`-i`, `--mode`, `--scenario`, `--duration`, `--progress`, `--progress-status`, `--id`, `--wait`, `--timeout` |
| `toastback wait ID` | wait for the answer (`--timeout`) |
| `toastback update ID` | move a progress bar: `--progress`, `--status`, `--title`, `--label` |
| `toastback notify-send …` | notify-send's command line |
| `toastback examples [NAME]` | list the example specs, or print one |
| `toastback validate FILE…` | check specs without showing them |
| `toastback log` | recent events and callback output (`-n N`, `--id ID`) |
| `toastback remove ID…`, `toastback clear` | take notifications away; their buttons stop working |
| `toastback doctor` | check the installation (`--json`) |
| `toastback install` | install or upgrade: `--wsl`, `--add-to-path`, `--quiet`, `--no-notify-send`, `--notify-send`, `--skill` (write this file to `~/.claude/skills/toastback/`), `--no-skill` |
| `toastback uninstall` | remove it all (`--purge` also deletes logs and pending notifications) |
| `toastback shim` | print the WSL shim (`--win-exe`) |
| `toastback skill` | print this file |

The shim passes `--distro`, `--user` and `--cwd` itself; you don't need them.
