# 000: M0 spikes

Date: 2026-10-14. Host: Windows 11 build 26300 (x64), WSL2 Fedora 44. Code: `src/spike.rs`
(the hidden `toastback spike …` subcommand), removed again in M1.

All five spikes passed, so the plan's primary designs stand and none of the fallbacks are needed.

| # | Question | Result |
|---|---|---|
| 1 | Cross-build a GUI-subsystem exe with manifest + icon + version info | **Yes.** `cargo xwin build` + `embed-resource` (llvm-rc) → `PE32+ (GUI)`, version info and icon present. Release size 771 KB (clap + windows). |
| 2 | stdout / stderr / exit code from WSL | **Yes**, through pipes (`id=$(…)`) and through a pty. A GUI-subsystem exe is enough, so no `toastback-cli.exe`. |
| 3 | Toast from an unpackaged exe with only the registry AUMID | **Yes.** `HKCU\Software\Classes\AppUserModelId\Toastback` (`DisplayName`, `IconUri`) is enough, no Start Menu shortcut needed. |
| 4 | Protocol-activated button runs `toastback.exe activate "<uri>"` | **Yes**, both from a toast button click and from `Start-Process toastback://…`. |
| 5 | TaskDialogIndirect with custom buttons | **Yes.** The manifest pulls in Common Controls v6. The selected button id comes back. |

## Findings that change the implementation

- **The URI gets a slash.** Windows passes `toastback://act?id=…` to the handler as
  `toastback://act/?id=…`. The URI parser in `activate.rs` has to accept both forms.
- **`ToastNotifier.Setting` fails** with `0x80070490` (Element not found) for a registry-only AUMID, even though
  `Show()` works. `doctor` can't use it to check whether notifications are enabled. Read the per-app value under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\Toastback` (`Enabled`=0 means off)
  instead, and treat a missing key as "enabled".
- **`cargo xwin test` forces `wine`** as the runner and ignores `.cargo/config.toml`. `just test` sets
  `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=/usr/bin/env` so tests run through WSL interop.
- lld warns LNK4099 because the xwin CRT has no PDBs. It's harmless and silenced with `/ignore:4099` in `.cargo/config.toml`.

## Still to confirm by eye (manual)

- No console window flashes when a toast button is clicked. GUI subsystem, so none is expected.
- The toast shows the name "toastback" and the bell icon from `IconUri`.
- The TaskDialog opens on top of other windows when started from a background process
  (`TDN_CREATED` → `HWND_TOPMOST` + `SetForegroundWindow`).
