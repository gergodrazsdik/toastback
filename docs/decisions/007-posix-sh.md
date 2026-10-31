# 007: no bash needed in WSL

Date: 2026-10-31. Changes the `bash -lc` of PLAN.md §4's callback table and the shim's first line.

`install.sh` and the shim are plain POSIX `sh`, so they run in distros without bash, such as Alpine.

A `{"wsl": "string"}` callback keeps `bash -lc` where bash exists, since existing specs may use its syntax, and falls
back to `sh -lc`. The choice has to be made inside the distro, so `wsl.exe --exec` starts a small `sh -c` script
(`exec::WSL_SHELL_SCRIPT`) that takes the string as an argument: it's never pasted into the script, so there's no second
round of parsing. It `exec`s the shell, so the exit code and the timeout's process tree stay the same.

The log's `callback_started` shows that script in front of the string. In a distro without bash, a string that relies
on bash-only features can fail. toastback doesn't refuse it: it can't tell from Windows, and BusyBox `ash` accepts some
of them, such as `[[ ]]`.
