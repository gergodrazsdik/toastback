# toastback

Async Windows toast notifications with callbacks, for WSL and scripts.

`toastback` shows a toast (or a native dialog) and **returns immediately**. When someone clicks a button,
even hours later from the notification center, Windows starts `toastback` again and it runs the command
attached to that button: in **WSL** (by default in the distro, user and directory that sent the notification) or
on **Windows**.

```bash
toastback -t "Backup finished" -m "42 GB in 12 min"      # simple alert, prints the notification id
toastback examples deploy | toastback                    # buttons that run commands
id=$(toastback --mode dialog -t "Reboot?" -m "Updates are installed")
```

- Single self-contained exe, no runtime. Windows 10 22H2+ / Windows 11, per-user install, no admin rights.
- Buttons run commands in WSL (`bash -lc` or an argv), cmd, PowerShell, any exe, or open a URL/file.
- Click a toast's text to open a details dialog with the same buttons, or use `mode: dialog` for prompts
  that must not be missed.
- Callback output, exit codes and timings are logged (`toastback log`). Optional "done"/"failed" toasts.

[Releases](https://github.com/gergodrazsdik/toastback/releases) follow [semantic versioning](https://semver.org);
what changed is in [CHANGELOG.md](CHANGELOG.md). The design is in [PLAN.md](PLAN.md) and
[docs/decisions/](docs/decisions/).

## Install

toastback always installs on the Windows side, for your user and without admin rights, even when you start the
install from WSL. To call it from WSL, it also needs a small **shim** script, `~/.local/bin/toastback`, in each distro
you use. Pick the method that matches how you work:

| Method | Best for | Shim goes to | Added to the Windows PATH |
|---|---|---|---|
| [winget](#winget) | Windows and WSL, with upgrades through winget | the default distro | yes |
| [`install.sh`](#from-wsl-installsh) | working mainly in one distro | that distro, for your user | with `--add-to-path` |
| [Zip](#zip) | no winget, or a specific version on Windows | the default distro | with `--add-to-path` |
| [From source](#from-source) | developing toastback | that distro, for your user | yes |

Whichever you use, `install` copies `toastback.exe` and `toastback-cli.exe` to `%LOCALAPPDATA%\toastback\bin`,
registers the `toastback://` protocol, the notification app id and an Apps & Features entry (HKCU only), writes the
shim and shows a test notification. `toastback doctor` checks everything.

### winget

```powershell
winget install GergoDrazsdik.Toastback
```

Then open a new terminal, so that it sees the new PATH. Upgrade with `winget upgrade GergoDrazsdik.Toastback`, which
refreshes the shims you already have, and remove it with `winget uninstall GergoDrazsdik.Toastback`. winget's
install shows no test notification; try `toastback-cli -t hello` instead.

### From WSL: `install.sh`

In an empty directory, download the installer and the checksums, check the installer and read it. `less` only opens
once the checksum is right (`install.sh: OK`):

```bash
cd "$(mktemp -d)"
base=https://github.com/gergodrazsdik/toastback/releases/latest/download
curl -fsSLO "$base/install.sh" && curl -fsSLO "$base/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS && less install.sh
```

Then, in the same directory, run it (add `--add-to-path` for `toastback-cli` in cmd and PowerShell):

```bash
bash install.sh
```

`install.sh` downloads `toastback.exe`, `toastback-cli.exe` and the release's `SHA256SUMS` into your Windows temp
folder, checks the exes against it, and runs `toastback.exe install` for your distro and user. Extra arguments go
to `install`. For a specific version, use `releases/download/v1.0.0` as `base` and run
`TOASTBACK_VERSION=v1.0.0 bash install.sh`. Release builds also carry a
[build provenance attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations):
`gh attestation verify toastback.exe -R gergodrazsdik/toastback` (likewise `toastback-cli.exe`, the zip and, in
releases after 1.0.0, `install.sh`) shows that GitHub Actions built the file from this repository.

### Zip

Download `toastback-x86_64.zip` from the [releases](https://github.com/gergodrazsdik/toastback/releases), extract it
anywhere, then in that folder run:

```powershell
.\toastback-cli.exe install --add-to-path
```

You can delete the extracted folder afterwards. To upgrade, do the same with the new zip.

### From source

In WSL, with [cargo-xwin](https://github.com/rust-cross/cargo-xwin), clang/lld/llvm and
[just](https://github.com/casey/just): `just setup` once, then `just install`.

### Which distros get the shim

Choose with `--wsl`. Without it, `install` uses the calling distro when run through `install.sh` or `just install`.
From Windows, a first install uses the default distro, and a later one (including `winget upgrade`) refreshes the
shims from the last install, so your choice stays.

| `--wsl` | Writes the shim to |
|---|---|
| `default` | the default distro (the one marked `*` in `wsl -l -v`) |
| `all` | every distro except Docker Desktop's (`docker-desktop*`), which are skipped with a note |
| a distro name | that distro, including Docker Desktop's if you name it; it must exist |
| `none` | nowhere, for Windows-only use (removes the shims of earlier installs too) |

If Docker Desktop's distro is your default, no shim is written and a warning asks you to choose one with `--wsl`. If
a shim you asked for with `--wsl` can't be written, `install` exits with an error; without `--wsl` it only warns, and
a later install tries again.

Other `install` options:

| Option | Meaning |
|---|---|
| `--add-to-path` | add the install dir to your user PATH, for `toastback-cli` in cmd/PowerShell |
| `--quiet` | skip the test notification |

### Add the shim to another distro

For a distro you installed later, or one the install skipped. **From inside that distro**, so that the shim is for your
user there (replace `<you>` with your Windows user name):

```bash
/mnt/c/Users/<you>/AppData/Local/toastback/bin/toastback.exe install --quiet \
  --wsl "$WSL_DISTRO_NAME" --distro "$WSL_DISTRO_NAME" --user "$(id -un)"
```

**Or from PowerShell,** for that distro's default user:

```powershell
& "$env:LOCALAPPDATA\toastback\bin\toastback-cli.exe" install --quiet --wsl Ubuntu-24.04
```

Existing shims stay, and later upgrades refresh the new one too. If `toastback` isn't found in a new terminal in that
distro, add `~/.local/bin` to your PATH there.

### Upgrade and uninstall

Upgrade the way you installed: re-running `install` replaces the exes. `toastback uninstall [--purge]` removes
everything, whichever way it was installed; so does Apps & Features. `--purge` also deletes logs and pending
notifications.

## Usage

```
toastback [show] [FILE | -] [-t TITLE] [-m BODY] [-d DETAILS] [-i ICON]
                 [--mode toast|dialog] [--scenario default|reminder|alarm|urgent] [--id ID]
```

The spec comes from `FILE`, from stdin (`-`, or when no `-t`/`-m` is given), or only from flags. Flags
override fields of the spec. `toastback` prints the notification id and exits:

| Exit code | Meaning |
|---|---|
| 0 | shown (the id is on stdout) |
| 2 | invalid spec or unreadable image |
| 3 | failed to show |
| 4 | not installed |

Other commands:

| Command | What it does |
|---|---|
| `toastback examples [NAME]` | list the built-in example specs, or print one (`> my.json` to start your own) |
| `toastback log [-n N] [--id ID]` | recent events: shown, clicked, callback output and exit codes |
| `toastback validate FILE...` | check specs (and their images) without showing them; exit 2 if any is invalid |
| `toastback remove ID...` / `toastback clear` | take notifications out of the notification center; their buttons stop working |
| `toastback doctor [--json]` | check install, registration, notification settings, WSL and the shim |
| `toastback shim [--win-exe PATH]` | print the WSL shim |
| `toastback --version` | version and git commit |

### From PowerShell or cmd

Use **`toastback-cli`** (after a winget install or `install --add-to-path`, or by its full path). It's the same program built as a
console app, so the shell waits for it, prints its output in order and sets `$LASTEXITCODE`. `toastback.exe`
is a GUI app (so that clicking a button never flashes a console window), and interactive shells don't wait
for those.

```powershell
$id = toastback-cli -t "Build done" -m "Release build finished in 4 min"
toastback-cli C:\specs\deploy.json; if ($LASTEXITCODE) { throw "toastback failed" }
```

### From systemd services, timers and cron (in WSL)

These work as is: call `~/.local/bin/toastback` by its full path (services don't get your shell's PATH). WSL sets
`WSL_DISTRO_NAME` only for processes it starts itself, so the shim takes the distro name from `wslpath -w /`
instead. Callbacks still run in the right distro, as the service's user, from the service's working directory.

```ini
# ~/.config/systemd/user/updates-check.service
[Service]
Type=oneshot
ExecStart=/bin/sh -c 'n=$(dnf -q check-update | grep -c .); [ "$n" -eq 0 ] || \
  %h/.local/bin/toastback -t "Updates available" -m "$n packages can be upgraded"'
```

### From Task Scheduler

Toasts need a user session: set the task to **Run only when user is logged on** (not "whether user is logged
on or not", which runs in session 0 where toasts can't be shown). Action: program
`%LOCALAPPDATA%\toastback\bin\toastback.exe`, arguments `C:\specs\backup-check.json`. The GUI exe opens no
console window.

## The spec

```jsonc
{
  "id": "deploy-1234",              // optional: generated if absent ([A-Za-z0-9._-], max 64, no leading -)
  "mode": "toast",                  // toast | dialog
  "title": "Deploy to production?", // required
  "body": "Build 1234 passed all checks.",
  "details": "Long text…",          // shown in a dialog when the toast body is clicked
  "icon": "/home/g/icons/rocket.png",   // info | warning | error | question, or a PNG/JPG, WSL or Windows path (copied to a cache)
  "image": null,                    // larger inline image
  "attribution": "CI",              // small grey line
  "scenario": "default",            // default | reminder | alarm | urgent (reminder/alarm stay on screen; need a button)
  "sound": "default",               // default | silent | alarm | reminder
  "expires": "3d",                  // how long it stays clickable: s, m, h, d (e.g. 1h30m). Default 3d (toast), 7d (dialog)
  "on_click": "details",            // body click: details | default | none (default: details if set, else default)
  "buttons": [                      // up to 5
    { "id": "deploy", "label": "Deploy", "style": "success",   // style: success | critical (Windows 11)
      "run": { "wsl": "./scripts/deploy.sh --env prod" } },
    { "id": "later", "label": "Not now" }                      // no run: uses "default", else just closes
  ],
  "default": { "wsl": "echo \"$TOASTBACK_ACTION\" >> ~/clicks.log" },   // handler for buttons without run
  "data": { "build": 1234 },        // any JSON, passed to callbacks as TOASTBACK_DATA
  "notify_on_error": true,          // "failed" toast when a callback fails (default: true)
  "notify_on_success": false,       // "done" toast when it succeeds (default: false)
  "context": { "distro": "Ubuntu-24.04", "user": "g", "cwd": "/home/g/app" }   // normally filled by the shim
}
```

Unknown fields are errors, so typos don't go unnoticed. Each notification handles **one** click: the toast is
removed, and later clicks (also while its callback still runs) are ignored and logged. Windows keeps a toast in
the notification center for 3 days at most, so a longer `expires` only matters for dialogs. A choice in a dialog
that opened before `expires` still counts for up to a day after it. Ids that Windows
reserves as device names (`CON`, `NUL`, `COM1`, …) are refused.

**Built-in icons:** `icon` can name one instead of an image file: `info`, `warning`, `error` or `question` (exact
lowercase; for a file called `warning`, write `./warning`). Toasts show toastback's own PNG of it as the logo, and
dialogs show the Windows icon. From the command line: `toastback -i error -t "Build failed"`.

### Callbacks (`run`)

Exactly one of:

| Form | Runs |
|---|---|
| `{"wsl": "string"}` | `wsl.exe -d <distro> -u <user> --cd <cwd> --exec bash -lc "<string>"`: your login profile, so your PATH |
| `{"wsl": ["argv", …]}` | the argv directly, no shell (and no login profile) |
| `{"cmd": "string"}` | `cmd.exe /d /s /c "<string>"` |
| `{"pwsh": "string"}` | PowerShell 7 (`pwsh`), else Windows PowerShell, via `-EncodedCommand` (no quoting issues) |
| `{"exe": ["C:\\path\\x.exe", "arg"]}` | the program directly |
| `{"open": "url-or-path"}` | the default app for a URL, file or folder (Windows `%VAR%`s like `%USERPROFILE%` are expanded in paths, not in URLs) |

Plus optional modifiers: `"cwd"`, `"env": {}`, `"distro"`, `"user"` (override the context) and `"timeout": "5m"`
(the callback, and everything it started, is killed after it; processes detached on purpose with `nohup` or
`setsid` in WSL survive). Windows-side callbacks run in your profile folder unless `cwd` is set. A relative `cwd` is
resolved against the directory you ran `toastback` in (from cmd or PowerShell: for Windows-side callbacks only),
and `~` means your home directory in `wsl` callbacks only. `cmd` (and an `exe` that is a `.bat` or `.cmd` script)
can't run in a WSL directory outside `/mnt/<drive>` (cmd.exe refuses UNC paths), so `toastback` rejects that spec;
use `pwsh`.
A `distro` other than yours runs as that distro's default user in its home directory, unless `user`/`cwd` say
otherwise (also when the spec's `context` names another distro). The log records each callback's command and the
names of its `env` variables, not their values.

A program a callback leaves running in the background (`start /b server.exe`, `Start-Process -NoNewWindow`,
`server &`) should send its output elsewhere (`> server.log 2>&1`): toastback reads the callback's output only until
2 seconds after the callback itself exits, and a program still writing to it afterwards may get "broken pipe"
errors.

Callbacks get `TOASTBACK_ID`, `TOASTBACK_ACTION` (the button id, or `__body`), `TOASTBACK_TITLE` and
`TOASTBACK_DATA` (the `data` JSON) as environment variables. For WSL callbacks they (and `env`) are passed
through `WSLENV`. toastback never pastes anything from the notification into a command line.

**Don't let your command re-parse untrusted text.** If a title, `data` or `env` value can come from text you
don't control (a script's output, an issue title, a file name), keep it a value, not code:

- **`cmd`: avoid `%TOASTBACK_TITLE%`, `%TOASTBACK_DATA%` and `%VAR%`s from `env`.** cmd expands `%VAR%` *before*
  it parses `&`, `|`, `>` and the rest, so a title like `x & del …` runs the second command. `%TOASTBACK_ID%` and
  `%TOASTBACK_ACTION%` are safe (letters, digits, `.` `_` `-` only). Use `pwsh` or `exe` to work with other values.
- **`wsl` (bash):** quote expansions, `"$TOASTBACK_TITLE"`, and never pass them to `eval`, `bash -c` or similar.
- **`pwsh`:** `$env:TOASTBACK_TITLE` is just a string. Don't hand it to `Invoke-Expression`.

### Examples

`toastback examples` lists the built-in examples, ready to copy and adapt: `simple`, `warning` (built-in icons),
`build` (open the log, re-run, replace by id), `deploy` (details, data, a WSL callback), `export` (Windows-side
callbacks), `reboot-dialog` (dialog mode) and `alarm`. Print one with `toastback examples NAME > my.json`, or show
it with `toastback examples NAME | toastback`. Callback output doesn't appear anywhere on screen: read it with
`toastback log`, or set `notify_on_success`.

[examples/](examples/) holds the same specs plus a few that need the repository or cover single features:
`images`, `default-handler`, `feedback` (done/failed toasts), `wsl-options` (argv, env, cwd, timeout) and
`windows-callbacks` (cmd, PowerShell, exe, open). Run them from the repository root: `just demo`, or
`toastback examples/<name>.json`.

## Limitations

- **Images:** local PNG or JPG files (WSL or Windows paths). Keep them small, under about 200 KB and 1024 px:
  Windows silently drops images it considers too large.
- **No dismiss callback.** Once `toastback` has exited, Windows tells nobody when a toast is dismissed or
  times out. Only clicks run callbacks.
- **Focus assist / Do not disturb** hides toast banners (they still land in the notification center). Use
  `"mode": "dialog"` or `"scenario": "alarm"` for prompts that must be seen.
- Buttons in the notification center appear once you expand the notification.
- Toasts can't be shown from session 0 (services, scheduled tasks that run "whether user is logged on or not").
- The first WSL callback after the distro has stopped takes a few seconds while WSL starts.
- **toastback isn't code-signed** (a deliberate choice, see [docs/decisions/004-m4.md](docs/decisions/004-m4.md)).
  With Windows 11's **Smart App Control** turned on, Windows may block unsigned apps it doesn't know yet ("An
  Application Control policy has blocked this file"; `install.sh` reports it). Smart App Control has no per-app
  exceptions. Without it, a browser download may still trigger SmartScreen on first run. `install` writes a fresh
  copy, so clicks don't trigger "downloaded from the internet" prompts; `install.sh` downloads with curl, which sets
  no download marker at all.
- Dialogs show only the built-in icons (`info`, `warning`, `error`, `question`), not an image file. With an image
  `icon` or none, a dialog shows the warning icon for `alarm`/`urgent` and the information icon otherwise.

## Troubleshooting

- `toastback doctor` checks the install, the protocol registration, whether notifications are enabled for
  toastback and whether your WSL shim is current.
- `toastback log` shows every click and callback, including stdout/stderr (last 8 KB each). The raw log
  is `%LOCALAPPDATA%\toastback\log\toastback.jsonl`.
- `uninstall` warns when a callback or dialog is still running from the install folder. Those files stay
  until it exits; the warning prints the command that removes the rest.
- A click that seems to do nothing: check `toastback log`. A button with no `run` and no `default` only closes
  the toast; this is by design.

## Development

Built in WSL for `x86_64-pc-windows-msvc` with cargo-xwin. `just` lists the recipes:

| Recipe | |
|---|---|
| `just build` | release build of both exes |
| `just check` | rustfmt, clippy (Windows target, warnings as errors), shellcheck, yamllint, actionlint |
| `just test` | unit tests on Linux and on Windows (through WSL interop), then `validate` on every example |
| `just install` | build and install for the current distro and on the Windows PATH |
| `just demo` / `just log` / `just doctor` | show every example / recent events / check the install |
| `just bump X.Y.Z` / `just release X.Y.Z` | release: open the version-bump PR / after merging it, tag and publish |
| `just winget-setup` / `just winget-test` | winget: check the tools / install this build through winget, check it and uninstall it |
| `just winget-manifest X.Y.Z` / `just winget-test-release X.Y.Z` | winget: validate a release's manifest / run the same test with it |
| `just winget-submit X.Y.Z` / `just winget-update X.Y.Z` | winget: open the first winget-pkgs PR / the PR for a later version |

Design notes and decisions are in [docs/decisions/](docs/decisions/); the manual test checklist is
[docs/manual-test.md](docs/manual-test.md).

## Contributing

Issues and pull requests are welcome. `main` only changes through pull requests whose CI (build, Windows tests,
smoke test) passes; CI for first-time and outside contributors starts after a maintainer approves it. Please add
a line under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for anything users would notice. Security problems:
see [SECURITY.md](SECURITY.md).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
