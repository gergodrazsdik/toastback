# 0001: Baseline: the plan's locked decisions

- Status: accepted, amended by [0003](0003-terminal-output.md) (a second exe) and [0013](0013-winget.md) (winget)
- Date: 2026-10-14

## Context and problem statement

toastback started from an implementation brief, now [docs/plan.md](../plan.md). Its §2 fixed the decisions that the
rest of the plan builds on, so that the build could go from an empty repo to a released v1 without reopening them.
This record makes them the first ADR, so later ADRs have something to amend.

## Considered options

The alternatives were weighed in the plan itself; the table in §2 records only the outcome.

## Decision outcome

Chosen: the decisions in [the plan's §2](../plan.md#2-decisions-locked), in short:

- **Rust** with Microsoft's `windows` crate; `x86_64-pc-windows-msvc` only, static CRT; built in WSL with
  `cargo xwin`; `just` as the task runner; edition 2024 on the latest stable toolchain.
- **Fire-and-forget.** The caller never waits for a click. The exit code says whether the notification was shown,
  stdout carries its id.
- **Buttons only**, no text or select inputs.
- **Protocol activation** through a per-user `toastback://` scheme in HKCU, no COM activator.
- **Callbacks** per button (`run`), plus an optional notification-level `default`.
- **Display modes** `toast` (the default) and `dialog` (a native TaskDialog); a body click can open a details dialog.
- **Self-installing exe** (`toastback.exe install`) that writes a WSL shim from a template embedded in the exe; a WSL
  one-liner (`install.sh`) wraps it. No installer or MSI.
- **GitHub Releases** for distribution, built by GitHub Actions on Linux and smoke-tested on a Windows runner.
- **Public repo, GPL-3.0-or-later**, with an SPDX identifier at the top of each source file.
- **`clap`** (derive) for the CLI.
- **Windows 10 22H2 or later and Windows 11**, per-user install, no admin rights.
- **Out of scope for v1:** a custom popup, a wait mode, inputs, arm64, updating or replacing a notification by id,
  scheduled reminders, code signing, scoop, self-update.

### Consequences

- Changing one of these needs the maintainer's approval and a new ADR that amends this one.
- Windows doesn't tell anyone when a toast is dismissed or times out once the sending process has exited, so there is
  no `on_dismiss`: only clicks run callbacks. That's a limitation by design, listed in the README.

## More information

The plan's §12 lists what may come later: inputs through a COM activator, arm64, update by id and progress bars,
scheduled reminders, a custom popup, scoop, self-update.
