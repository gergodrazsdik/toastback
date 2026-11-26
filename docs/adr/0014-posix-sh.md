# 0014: No bash needed in WSL

- Status: accepted
- Date: 2026-10-31

## Context and problem statement

`install.sh`, the WSL shim and `{"wsl": "string"}` callbacks all needed bash ([0004](0004-running-callbacks.md),
[0011](0011-shim-without-distro-name.md)). Some distros, such as Alpine, don't ship it.

## Considered options

1. Require bash, and say so.
2. Plain POSIX `sh` for `install.sh` and the shim; for string callbacks, `sh -lc` everywhere.
3. Plain POSIX `sh` for `install.sh` and the shim; for string callbacks, `bash -lc` where bash exists and `sh -lc`
   otherwise.

## Decision outcome

Chosen option: 3. `install.sh` and the shim are ours, so they can be plain `sh`. String callbacks are the users', and
existing specs may use bash syntax, so they keep `bash -lc` where bash exists.

The choice has to be made inside the distro, so `wsl.exe --exec` starts a small `sh -c` script
(`exec::WSL_SHELL_SCRIPT`) that takes the string as an argument. The string is never pasted into the script, so there's
no second round of parsing. The script `exec`s the shell, so the exit code and the timeout's process tree stay the
same.

### Consequences

- `install.sh` runs with `sh install.sh` (`bash install.sh` still works), and uses `wget` when there's no `curl`.
- The log's `callback_started` shows that script in front of the string.
- In a distro without bash, a string that relies on bash-only features can fail. toastback doesn't refuse it: it can't
  tell from Windows, and BusyBox `ash` accepts some of them, such as `[[ ]]`.
