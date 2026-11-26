# 0004: Running callbacks: `wsl.exe --exec`, encoded PowerShell, raw cmd, one click per notification

- Status: accepted, amended by [0014](0014-posix-sh.md) (the shell for WSL strings)
- Date: 2026-10-14

## Context and problem statement

A callback is a command attached to a button. It runs in WSL or on Windows, and its string has to arrive exactly as
the spec wrote it, whatever quotes, `$` or UTF-8 it contains. The plan's callback table (§4) named the commands; M1
found where they mangle the string, and had to settle the working directory, timeouts and repeated clicks.

## Considered options

1. WSL: `wsl.exe … -- bash -lc "<string>"` (the plan) or `wsl.exe … --exec bash -lc "<string>"`.
2. PowerShell: the string as a `-Command` argument, or `-EncodedCommand`.
3. cmd: arguments quoted by Rust's rules, or the string appended verbatim.
4. Repeated activations (a second click, a replayed URI, the details dialog): run each, or run the first only.

## Decision outcome

Chosen:

- **`wsl.exe … --exec bash -lc "<string>"`.** With `--`, wsl.exe hands the rest of its command line to the user's
  login shell, which parses it a second time and mangles quotes and `$`. With `--exec`, the argv arrives as is, and
  `bash -l` still loads the login profile. Tested with spaces, `"`, `'`, `$`, `&` and UTF-8.
- **`pwsh -EncodedCommand`** (base64 of UTF-16LE), so PowerShell's command-line quoting never applies. Without
  `pwsh.exe`, `powershell.exe` runs instead.
- **`cmd.exe /d /s /c "<string>"`**, appended verbatim (`raw_arg`). cmd.exe doesn't follow the MSVC argv rules, so
  Rust's quoting would corrupt the string.
- **One click per notification.** The pending file is marked `done` before the callback runs, by a claim under a file
  lock (`Store::claim`). Later activations are logged as `rejected: already handled`.

### Consequences

- Windows forms run in `%USERPROFILE%` unless the run sets `cwd`. A WSL path as `cwd` is converted like an image path
  (`/mnt/c/x` becomes `C:\x`).
- A timeout kills `wsl.exe`, and that ends the Linux process too: no orphaned `sleep`. Rechecked on 2026-10-14 with
  WSL 2, since the callback runs in a job object: a backgrounded child (`(sleep 8; touch f) & sleep 30` with a 3 s
  timeout) dies too. Only processes detached on purpose (`nohup`, `setsid`) outlive it.

## More information

[0014](0014-posix-sh.md) replaced `bash -lc` with a small `sh` script that picks bash or sh inside the distro.
