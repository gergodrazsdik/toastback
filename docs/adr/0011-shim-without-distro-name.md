# 0011: Shim v2: the distro from `wslpath` when `WSL_DISTRO_NAME` is missing

- Status: accepted, amended by [0014](0014-posix-sh.md) (the shim runs in `sh`)
- Date: 2026-10-14

## Context and problem statement

The WSL shim passes the caller's distro to the exe, so WSL callbacks run where the notification came from
([0005](0005-spec-and-cli.md)). WSL sets `WSL_DISTRO_NAME` only for processes it starts itself; systemd services,
timers and cron jobs don't have it. The v1 shim passed `--distro ""`, and those callbacks ran in the default distro.

## Considered options

1. Document a workaround: set `WSL_DISTRO_NAME` for the user's services (for example in
   `~/.config/environment.d/`).
2. Fall back to a source that doesn't need the variable.

## Decision outcome

Chosen option: "fall back", because a notification from a service should behave like one from a terminal without
extra setup. The v2 shim uses `WSL_DISTRO_NAME` when it's set, and otherwise `wslpath -w /`, which returns
`\\wsl.localhost\<distro>\`. Checked in a `systemd-run --user` service with `UnsetEnvironment=WSL_DISTRO_NAME`: the
notification's context and the callback both landed in the right distro.

### Consequences

- User services also lack the Windows PATH entries (`cmd.exe` isn't found). That doesn't matter to the shim: it runs
  the exe by its absolute path, and interop (binfmt `WSLInterop`) works.
- `install` now also drops recorded shims for distros that no longer exist, and `uninstall` skips them with a note
  (after `wsl --unregister`).
