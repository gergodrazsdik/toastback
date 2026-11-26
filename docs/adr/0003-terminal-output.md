# 0003: Terminal output: attach the parent console, plus a console-subsystem `toastback-cli.exe`

- Status: accepted
- Date: 2026-10-14

## Context and problem statement

toastback.exe is a GUI-subsystem exe, so Windows never opens a console window when a button click or Task Scheduler
starts it ([0001](0001-baseline.md)). The M0 spike ([0002](0002-platform-spikes.md)) only tested captured output and a
Python pty in a session without a Windows console. M1's manual testing found that in a real interactive terminal the
exe gets **no std handles at all**: `GetStdHandle` fails, so `toastback …` printed nothing, not even the id or `log`.
A probe showed this in a WSL terminal and in PowerShell.

A second problem: interactive cmd and PowerShell don't wait for GUI-subsystem exes. The output lands after the next
prompt, and `$LASTEXITCODE` isn't set. Only a captured call (`$id = toastback.exe …`) waits.

## Considered options

1. Attach the missing streams to the parent's console at startup.
2. A second, console-subsystem exe, `toastback-cli.exe` (the plan's fallback, §3).
3. Both: the first for WSL, the second for interactive cmd and PowerShell.

## Decision outcome

Chosen option: "both". Attaching the console fixes WSL terminals without a second exe; cmd and PowerShell can't be
fixed from inside a GUI exe, so `toastback-cli.exe` was added at the maintainer's request in M3.

**Attaching** (`src/console.rs`): at startup, except in `activate`, every stream without a handle is pointed at the
parent's console (`AttachConsole(ATTACH_PARENT_PROCESS)`, `CONIN$`/`CONOUT$`, `SetStdHandle`). Pipes are left alone,
so captured output never also goes to the console. The console that WSL relays to has `DISABLE_NEWLINE_AUTO_RETURN`
set, so a bare `\n` didn't return to column 0 (staircase output); `console.rs` clears the flag while toastback runs
and restores the original mode on exit, because the mode belongs to the terminal's shared screen buffer. That's also
why `main` returns on clap errors instead of calling `exit()`.

**Two binaries from one crate:** the crate is a library (`src/lib.rs`, `toastback::main()`) with two thin binaries:

- `toastback.exe` (`src/main.rs`, GUI subsystem): the protocol handler, the dialog process and the WSL shim's target.
- `toastback-cli.exe` (`src/bin/toastback-cli.rs`, console subsystem): for interactive cmd and PowerShell.

### Consequences

- Both exes get the same resources (`embed-resource` links them into every binary). `install` copies both, and the
  registration always points at `toastback.exe`. A dialog started from `toastback-cli.exe` runs under its sibling
  `toastback.exe` (`install::gui_exe`).
- `console.rs` also clears `DISABLE_NEWLINE_AUTO_RETURN` on a console the process already owns, so the CLI exe doesn't
  print a staircase in a WSL terminal either.
- The release zip carries both exes, and `toastback-cli.exe` is a separate release asset. `install.sh` needs only
  `toastback.exe` for WSL, but installs both, so `--add-to-path` users get the CLI.
