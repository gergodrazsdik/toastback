# 000: M0 spikes

Date: 2026-10-14. Host: Windows 11 build 26300 (x64), WSL2 Fedora 44. Code: `src/spike.rs`
(the hidden `toastback spike …` subcommand), removed again in M1.

All five spikes passed, so the plan's primary designs stand and none of the fallbacks are needed.

| # | Question | Result |
|---|---|---|
| 1 | Cross-build a GUI-subsystem exe with manifest + icon + version info | **Yes.** `cargo xwin build` + `embed-resource` (llvm-rc) → `PE32+ (GUI)`, version info and icon present. Release size 771 KB (clap + windows). |
| 2 | stdout / stderr / exit code from WSL | **Partly. Corrected after M1, see below.** Pipes (`id=$(…)`) and the exit code work. In an interactive terminal, nothing printed showed up. |
| 3 | Toast from an unpackaged exe with only the registry AUMID | **Yes.** `HKCU\Software\Classes\AppUserModelId\Toastback` (`DisplayName`, `IconUri`) is enough, no Start Menu shortcut needed. |
| 4 | Protocol-activated button runs `toastback.exe activate "<uri>"` | **Yes**, both from a toast button click and from `Start-Process toastback://…`. |
| 5 | TaskDialogIndirect with custom buttons | **Yes.** The manifest pulls in Common Controls v6. The selected button id comes back. |

## Correction: terminal output (found in M1 manual testing)

The spike only tested captured output, plus a Python pty in a session that has no Windows console. In a real
interactive terminal, the GUI-subsystem exe gets **no std handles at all**. `GetStdHandle` fails, so `toastback …`
printed nothing (no id, no `log`). A probe (`__probe`, since removed) showed this in a WSL terminal and in
PowerShell. In both, `AttachConsole(ATTACH_PARENT_PROCESS)` followed by writing to `CONOUT$` works. Redirected
streams always arrive as pipe handles.

Fix (`src/console.rs`): at startup, except in `activate`, every stream without a handle is pointed at the
parent's console (`CONIN$`/`CONOUT$` + `SetStdHandle`). Pipes are left alone, so captured output never also
goes to the console. The console WSL relays to has `DISABLE_NEWLINE_AUTO_RETURN` set, so a bare
`\n` didn't return to column 0 (staircase output). `console.rs` clears that flag while toastback runs and
restores the original mode on exit, because the mode belongs to the terminal's shared screen buffer.
That's also why `main` returns on clap errors instead of calling `exit()`. No `toastback-cli.exe` is needed for WSL.

Remaining caveat: interactive **cmd/PowerShell don't wait for GUI-subsystem exes**. The output lands after the
next prompt, and `$LASTEXITCODE` isn't set. `$id = toastback.exe …` (captured) waits. If interactive Windows-side
use matters, the plan's fallback `toastback-cli.exe` (console subsystem) is still an option.

## Findings that change the implementation

- **The URI gets a slash.** Windows passes `toastback://act?id=…` to the handler as
  `toastback://act/?id=…`. The URI parser in `activate.rs` has to accept both forms.
- **`ToastNotifier.Setting` fails before the first toast.** For a registry-only AUMID it returns `0x80070490`
  (Element not found) until the app has shown a toast. After that it returns `Enabled`, presumably once Windows has
  created the app's notification settings entry. `doctor` treats that error as "no toast shown yet", not as broken.
  If it needs an answer before the first toast, it can read
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\Toastback` (`Enabled`=0 means off).
- **`cargo xwin test` forces `wine`** as the runner and ignores `.cargo/config.toml`. `just test` sets
  `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=/usr/bin/env` so tests run through WSL interop.
- lld warns LNK4099 because the xwin CRT has no PDBs. It's harmless and silenced with `/ignore:4099` in `.cargo/config.toml`.

## Confirmed by eye (2026-10-14, rerun of spikes 3–5)

- No console window flashes when a toast button is clicked.
- The toast shows the name "toastback" and the bell icon from `IconUri`.
- The TaskDialog, started from a background process (WSL), opens on top of other windows
  (`TDN_CREATED` → `HWND_TOPMOST` + `SetForegroundWindow`).
