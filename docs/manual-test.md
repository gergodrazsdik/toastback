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

## Dialogs

17. `toastback examples/reboot-dialog.json` returns at once, and the dialog appears on top with the warning icon and
    three command links. Click into another window first: the dialog stays on top. Pick **Reboot now**, and the log
    shows `pretend reboot: now`.
18. `toastback --mode dialog -t "Close me"` shows a dialog with a Close button. Esc logs `dialog_closed … (closed)`
    and runs no callback.

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

## Uninstall

26. `toastback uninstall`: the shim, the registry keys, the PATH entry and `bin\` are gone, and a new terminal no longer
    finds `toastback-cli`. **(CI: the registry keys)**
27. `just install` again, then `toastback uninstall --purge`: `%LOCALAPPDATA%\toastback` is gone.
28. `just install`, then start a long callback (for example **Snooze** in `examples/alarm.json`) and run `toastback uninstall` while
    it's running. It warns with the callback's pid, and only `bin\toastback.exe` is left. Once the callback is done, the
    printed command removes the rest.
