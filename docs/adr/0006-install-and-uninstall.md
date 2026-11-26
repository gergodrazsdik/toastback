# 0006: Install and uninstall: recorded shims, marked files, PATH, uninstall while running

- Status: accepted
- Date: 2026-10-14

## Context and problem statement

`toastback.exe install` copies itself to `%LOCALAPPDATA%\toastback\bin`, registers the app id and the protocol in
HKCU and writes a WSL shim into one or more distros ([0001](0001-baseline.md)). `uninstall` has to undo exactly that,
without deleting files it didn't write, and still work when a toastback process is running from the folder it
deletes. M1 and M3 settled the details.

## Considered options

1. What `install.json` records about shims: distro names, or `{distro, user}` pairs.
2. Which shims `uninstall` deletes: every file at the shim's path, or only files carrying the shim's marker.
3. Running toastback processes during `uninstall`: refuse, or remove what it can and say how to finish.
4. The PATH check: the exe's own environment, or the user's login shell.

## Decision outcome

Chosen:

- **`install.json` records `shims: [{distro, user}]`.** The calling distro gets the calling user's shim, and
  `uninstall` removes it from the same place.
- **Only files with the `# toastback WSL shim` marker are deleted.** When the running exe is inside the folder being
  deleted, a detached `cmd.exe` removes the folder once the process has exited.
- **Remove what it can.** A callback that hasn't finished, or an open dialog, keeps its `toastback.exe` running from
  `bin\`, and a running exe can't be deleted. `uninstall` lists such processes first (a toolhelp snapshot, then each
  one's image path, keeping those under the folder it deletes, excluding itself) and warns with their PIDs. It still
  removes everything else: shims, registry, PATH and every file not in use, then prints the command that removes the
  rest once they've exited.
- **The PATH check runs the user's login shell** (`$SHELL -lc`), so zsh users with PATH in `.zshenv` aren't warned for
  no reason.

### Consequences

- `install --add-to-path` (M3) appends `bin\` to `HKCU\Environment\Path`, keeping it `REG_EXPAND_SZ`, matching
  case-insensitively and ignoring a trailing `\`, then broadcasts `WM_SETTINGCHANGE`. `uninstall` removes it again
  if `install.json` says `install` added it.
- `--wsl all` lists distros with `wsl -l -q`, decoding UTF-16, or UTF-8 when `WSL_UTF8=1` is set.

## More information

Tested live: `uninstall --purge` during a 30 s callback leaves only the locked exe, and running that exe's
`uninstall --purge` afterwards removes the folder.

Later changes to install are in [0011](0011-shim-without-distro-name.md) (shim v2, distros that no longer exist) and
[0013](0013-winget.md) (the Apps & Features entry, `--wsl default`, `--quiet`).
