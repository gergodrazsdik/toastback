# 0007: Dialogs: TaskDialog mapping, on top, a detached dialog process, one callback path

- Status: accepted
- Date: 2026-10-14

## Context and problem statement

`mode: dialog` shows a native TaskDialog instead of a toast, for prompts that must not be missed, and a toast's body
click can open a details dialog with the same buttons ([0001](0001-baseline.md)). The plan named the API; M2 had to
map the spec onto it, keep the caller from waiting, and make the dialog visible when a background process starts it.

## Considered options

1. Custom icons: load the spec's `icon` image (needs WIC to turn a PNG into an HICON), or use the standard icons only.
2. Showing the dialog: in the `show` process, or in a detached `__dialog <id>` process.
3. Callback handling: a separate path for dialogs, or the same `finish` path as toast buttons.

## Decision outcome

Chosen:

- **Mapping:** title to the window title; body to the main instruction (the title again when there's no body);
  details to the content. Buttons become custom TaskDialog buttons, shown as command links when there are three or
  more. Without buttons the dialog gets a plain **Close**. Esc, X and Close all count as "closed", with no callback.
- **Standard icons only:** `alarm` and `urgent` show the warning icon, everything else the information icon. The
  window and taskbar icon is the exe's bell (resource 1, set with `WM_SETICON`). Custom images can be added later if
  they're wanted.
- **On top:** on `TDN_CREATED` the dialog is made `HWND_TOPMOST` and asks for the foreground. If Windows refuses (the
  dialog wasn't started by a click), the taskbar button flashes.
- **A detached `__dialog <id>` process.** `show` starts it with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`, and
  with `CREATE_BREAKAWAY_FROM_JOB` when the caller's job allows it. Before spawning, `show` marks its own std handles
  as non-inheritable; otherwise the child could hold WSL interop's pipes open and the WSL side would wait for the
  dialog.
- **One callback path:** a toast button, a body click (`default` or the details dialog) and a dialog choice all end
  in the same `finish` and `run_callback`. The log shows `dialog_shown` and `dialog_closed` (with the chosen button,
  or `(closed)`), then the usual `callback_*` events.

### Consequences

`toastback examples/reboot-dialog.json` returns in 0.19 s. The dialog stays on top while another window has focus.

## More information

All of M2's acceptance criteria were checked by hand on Windows 11 build 26300.
