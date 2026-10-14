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
