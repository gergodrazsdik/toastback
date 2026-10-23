# Manual test checklist

Run before a release, on Windows 11 x64 (and Windows 10 22H2 if available), from the repository root in
WSL. Each step lists what to expect. Callback output appears in `toastback log`. Steps marked **(CI)** are also
covered by the smoke job, so here they only need to be checked once.

## Install

1. `toastback uninstall --purge` (if installed), then `just install`.
   - It prints the bin dir and "wrote ~/.local/bin/toastback in <distro>", and a "toastback installed" toast appears.
2. `toastback doctor` shows every line `ok`, exit 0. **(CI: `doctor --json`)**
3. `toastback --version` shows the version and commit. **(CI)**
4. In a terminal: `toastback -t hi -m there` prints an id on its own line (no staircase, no `%` after it).

## Toasts and callbacks

5. `toastback examples/simple.json` shows a toast with title, body and attribution. Nothing happens on click.
6. `toastback examples/images.json` shows the bell as the app logo and as an inline image.
7. `toastback examples/deploy.json`, then click **Deploy**.
   - The log shows `callback_finished … deploy exit 0` with the distro, user and repo cwd, and `data={"build":1234}`.
8. The same, but let the toast time out, then expand it in the notification center (Win+N) and click **Deploy**.
   - Same result.
9. `toastback examples/deploy.json`, then click **Open logs**: the GitHub page opens. Show it again and click **Not now**:
   the toast closes and nothing is logged.
10. `toastback examples/deploy.json`, then click the toast **body**: the details dialog opens with the same buttons.
    Pick **Deploy** and it runs.
11. `toastback examples/default-handler.json`, then click **Green**: a "Green: done" toast appears, and
    `tail -1 ~/toastback-colors.log` shows `green`.
12. `toastback examples/feedback.json`, then click **Fail**: a "Fail: failed" toast with `disk full`. Repeat with
    **Time out**: a "timed out after 3.0s" toast. Repeat with **Succeed**: a "Succeed: done" toast.
13. `toastback examples/wsl-options.json`, then click each button: the log shows the `/tmp` listing, and
    `hello from /etc`.
14. `toastback examples/windows-callbacks.json`, then click each button. **cmd** and **PowerShell** show
    "done" toasts. **Notepad** opens Notepad. On Windows 11 its "done" toast appears at once, because `notepad.exe` only
    starts the Store app and exits. On Windows 10 it appears when Notepad closes. **Open folder**
    opens `%LOCALAPPDATA%\toastback` in Explorer.
15. `toastback examples/alarm.json`: the toast stays on screen with a looping sound until a button is chosen.
16. Text: `toastback -t "őű 🔔" -m "Ünïcödé ✓"` displays correctly.
38. `toastback examples/status-icons.json` shows the amber warning triangle as the logo. Then
    `for i in info warning error question; do toastback -i $i -t $i; done`: each toast shows its icon, sharp, in
    both the light and the dark Windows theme.

## Dialogs

17. `toastback examples/reboot-dialog.json` returns at once, and the dialog appears on top with the warning icon and
    three command links. Click into another window first: the dialog stays on top. Pick **Reboot now**, and the log
    shows `pretend reboot: now`.
18. `toastback --mode dialog -t "Close me"` shows a dialog with a Close button. Esc logs `dialog_closed … (closed)`
    and runs no callback.
39. `for i in info warning error question; do toastback --mode dialog -i $i -t $i; done` opens four dialogs with the
    information, warning, error and question icons (the question icon is sharp at 150 % scaling too). Warning and
    error play their system sounds. `toastback --mode dialog -i res/icon.png -t path` shows the
    information icon.

## Commands

19. `toastback validate examples/*.json`: all `ok`. `echo '{"title":"x","scenario":"alarm"}' | toastback validate -`
    gives FAIL, exit 2. **(CI)**
20. `id=$(toastback -t "remove me")`, then `toastback remove $id`: the toast disappears from the notification
    center. `toastback remove nope` exits 1.
21. Show two toasts, then `toastback clear`: both disappear, and the count is printed.
22. A forged click: `powershell.exe -c 'Start-Process "toastback://act?id=x&a=y&t=z"'` is logged as `rejected`.

## Windows side

23. In PowerShell: `& "$env:LOCALAPPDATA\toastback\bin\toastback-cli.exe" -t "from pwsh"` prints the id before the next
    prompt, and `$LASTEXITCODE` is 0. `… toastback-cli.exe nope.json` makes `$LASTEXITCODE` 2.
24. `toastback.exe install --wsl none --add-to-path` (from WSL: `toastback install --wsl none --add-to-path`):
    in a **new** PowerShell, `toastback-cli --version` works. `toastback doctor` shows `user PATH ok`.
25. Task Scheduler: a task "Run only when user is logged on" with action
    `%LOCALAPPDATA%\toastback\bin\toastback.exe -t "scheduled"` shows the toast and no console window.

29. From a systemd user service without the distro variable:
    `systemd-run --user --wait --pipe -p UnsetEnvironment=WSL_DISTRO_NAME ~/.local/bin/toastback -t from-systemd`
    shows the toast, and `toastback log` / the pending context names the right distro. The same with the variable
    set but empty: `-p Environment=WSL_DISTRO_NAME=`.

## Edge cases

30. **Double click:** `examples/deploy.json` has details, so its body opens the details dialog. Show it and read its
    token from `%LOCALAPPDATA%\toastback\pending\<id>.json`. Click the body to open the dialog. The clicked toast
    leaves the notification center, so click its **Deploy** from a terminal while the dialog is open:
    `toastback.exe activate "toastback://act?id=<id>&a=deploy&t=<token>"`. Then pick **Deploy** in the dialog.
    `toastback log` shows one `callback_started` and a `rejected … already handled`. (Without a desktop: start two
    such `activate` commands at once.)
31. **Timeout kills the tree:** `{"cmd": "ping -n 600 127.0.0.1", "timeout": "5s"}` gives the "timed out" toast after
    about 5 s, and Task Manager shows no `PING.EXE` left. `{"cmd": "start \"\" notepad"}` finishes at once, and
    Notepad stays open. `{"wsl": "(sleep 8; touch /tmp/tb) & sleep 30", "timeout": "3s"}`: no `/tmp/tb` appears.
    A callback `{"pwsh": "toastback-cli --mode dialog -t keep; Start-Sleep 60", "timeout": "5s"}`: the dialog stays
    open after the timeout.
32. **cmd in a WSL directory:** `toastback` with a `cmd` button and `"cwd": "/home/<you>"` exits 2 with "cmd can't run
    in \\wsl.localhost\…", and so does an `exe` button that runs a `.cmd` file. A relative `"cwd": "sub"` on a `wsl`
    button runs in `$PWD/sub`.
33. **Text starting with a dash:** `toastback -t cmake -m "-- Build files have been written"` shows that body.
34. **Windows encodings:** in PowerShell 5.1, `@{title='Jörg'} | ConvertTo-Json | Set-Content -Encoding UTF8 a.json`
    and `@{title='x'} | ConvertTo-Json > b.json` (UTF-16): `toastback-cli validate a.json b.json` says `ok` twice.
35. **Re-show while a callback runs:** a spec with `"id": "standup"` and a button
    `{"wsl": "sleep 5 && toastback ~/standup.json"}`: click it; the new toast stays after the first callback ends.
36. **Closed pipe:** `toastback log -n 1000 | head -1` prints one line and no error; the terminal still returns
    to column 0 after a newline.
37. **Secrets stay out of the log:** a button with `"env": {"SECRET": "s3cr3t"}`, clicked: `grep -c s3cr3t
    "$(wslpath "$(cmd.exe /c 'echo %LOCALAPPDATA%' 2>/dev/null | tr -d '\r')")/toastback/log/toastback.jsonl"` is 0.
    (On a machine that ran 1.0.0, delete that `log` folder first: 1.0.0 logged the values.)

## Uninstall

26. `toastback uninstall`: the shim, the registry keys, the PATH entry and `bin\` are gone, and a new terminal no longer
    finds `toastback-cli`. **(CI: the registry keys)**
27. `just install` again, then `toastback uninstall --purge`: `%LOCALAPPDATA%\toastback` is gone.
28. `just install`, then start a long callback (for example **Snooze** in `examples/alarm.json`) and run `toastback uninstall` while
    it's running. It warns with the callback's pid, and only `bin\toastback.exe` is left. Once the callback is done, the
    printed command removes the rest.
