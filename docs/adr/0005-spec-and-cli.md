# 0005: Spec and CLI: the spec's context wins, `show` is the default command, WSL paths

- Status: accepted
- Date: 2026-10-14

## Context and problem statement

The WSL shim always passes the caller's context (`--distro`, `--user`, `--cwd`) to the exe, and a spec can name a
context too. The CLI has subcommands but is mostly used as `toastback [file]`. And a spec file named from WSL has a
WSL path. M1 had to settle how these combine, where the plan left gaps.

## Considered options

1. Context: the shim's flags override the spec, or the spec overrides the shim's flags.
2. Default command: clap's `args_conflicts_with_subcommands`, or inserting `show` when the first argument after the
   context flags isn't a subcommand.
3. Spec file paths: taken as Windows paths, or resolved like image paths against the caller's cwd.

## Decision outcome

Chosen:

- **A context written in the spec wins over the shim's flags.** The shim always passes them, so otherwise a spec could
  never choose its own context. The quick flags (`-t`, `-m`, …) still override spec fields.
- **`show` is inserted** when the first argument after the context flags isn't a subcommand, because
  `args_conflicts_with_subcommands` broke `--distro D install`, the form the shim uses.
- **Spec file paths from WSL** (`toastback examples/deploy.json`) are resolved like image paths, against the caller's
  cwd.

### Consequences

- A relative path with no caller context (the exe run without the shim) is used as is, relative to the process's cwd
  (since M3; it was an error before). This lets `toastback.exe validate examples/*.json` work straight from WSL.
- Validation messages name enum values in their JSON spelling (`"alarm"`, not `Alarm`).
- `open` targets get `%VAR%` expanded (`ExpandEnvironmentStringsW`); ShellExecute doesn't do that itself.
