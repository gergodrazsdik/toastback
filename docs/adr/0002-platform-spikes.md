# 0002: Platform spikes: registry-only app id, protocol activation, TaskDialog, cross-built GUI exe

- Status: accepted
- Date: 2026-10-14

## Context and problem statement

The plan's design rests on Windows behaving in ways that weren't verified: that a cross-built GUI-subsystem exe works,
that an unpackaged exe can show toasts, that a button can start it through a URI, and that a TaskDialog with custom
buttons works. Milestone M0 tested each with a hidden `toastback spike …` subcommand (`src/spike.rs`, removed in M1),
on Windows 11 build 26300 (x64) with WSL2 Fedora 44.

## Considered options

For each spike, the plan's primary design or its fallback:

1. Cross-build a GUI-subsystem exe with manifest, icon and version info.
2. stdout, stderr and the exit code reach a WSL caller; fallback: a console-subsystem `toastback-cli.exe`.
3. A toast from an unpackaged exe with only a registry app id (AUMID); fallback: a Start Menu shortcut.
4. A protocol-activated button runs `toastback.exe activate "<uri>"` with no console flash.
5. A TaskDialog with custom buttons.

## Decision outcome

Chosen: the primary design for all five, because every spike passed:

| # | Result |
|---|---|
| 1 | `cargo xwin build` with `embed-resource` (llvm-rc) gives a `PE32+ (GUI)` exe with version info and icon. Release size 771 KB with clap and windows. |
| 2 | Pipes (`id=$(…)`) and the exit code work. Interactive terminals didn't, which M1 found later: see [0003](0003-terminal-output.md). |
| 3 | `HKCU\Software\Classes\AppUserModelId\Toastback` with `DisplayName` and `IconUri` is enough; no shortcut needed. |
| 4 | Works from a button click and from `Start-Process toastback://…`. No console window flashes. |
| 5 | The manifest pulls in Common Controls v6, and the selected button's id comes back. |

### Consequences

Findings that changed the implementation:

- **Windows adds a slash to the URI.** It passes `toastback://act?id=…` to the handler as `toastback://act/?id=…`, so
  the parser in `activate.rs` accepts both forms.
- **`ToastNotifier.Setting` fails before the first toast.** For a registry-only app id it returns `0x80070490`
  (Element not found) until the app has shown a toast; after that it returns `Enabled`, presumably once Windows has
  created the app's notification settings entry. `doctor` treats that error as "no toast shown yet", not as broken.
  If it ever needs an answer before the first toast, it can read
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\Toastback` (`Enabled`=0 means off).
- **`cargo xwin test` forces `wine` as the runner** and ignores `.cargo/config.toml`. `just test` sets
  `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=/usr/bin/env`, so the tests run through WSL interop.
- **lld warns LNK4099** because the xwin CRT has no PDBs. It's harmless and silenced with `/ignore:4099` in
  `.cargo/config.toml`.

## More information

Confirmed by eye on 2026-10-14, in a rerun of spikes 3 to 5: no console window flashes when a button is clicked; the
toast shows the name "toastback" and the bell icon from `IconUri`; the TaskDialog, started from a background process
in WSL, opens on top of other windows (`TDN_CREATED`, then `HWND_TOPMOST` and `SetForegroundWindow`).
