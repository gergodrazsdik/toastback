// SPDX-License-Identifier: GPL-3.0-or-later
//! Native dialogs (TaskDialogIndirect): for `mode: dialog`, and the details dialog a toast body
//! click opens. The model is pure and tested on the host; showing it is Windows-only.

use crate::spec::{Scenario, Spec};

/// Custom button ids start here; `id - BUTTON_BASE` is the index into `Model::buttons`.
pub const BUTTON_BASE: i32 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Information,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub window_title: String,
    pub instruction: String,
    pub content: Option<String>,
    /// (button id, label)
    pub buttons: Vec<(String, String)>,
    pub icon: Icon,
    pub command_links: bool,
}

/// title → window title, body → main instruction (the title if there's no body), details → content.
pub fn model(spec: &Spec) -> Model {
    Model {
        window_title: spec.title.clone(),
        instruction: spec.body.clone().unwrap_or_else(|| spec.title.clone()),
        content: spec.details.clone(),
        buttons: spec
            .buttons
            .iter()
            .map(|b| (b.id.clone(), b.label.clone()))
            .collect(),
        icon: match spec.scenario {
            Scenario::Alarm | Scenario::Urgent => Icon::Warning,
            Scenario::Default | Scenario::Reminder => Icon::Information,
        },
        // three or more buttons read better as a vertical list of command links
        command_links: spec.buttons.len() >= 3,
    }
}

/// Map TaskDialog's result to a button id. Cancel/close (or anything unknown) is `None`.
pub fn chosen(model: &Model, result: i32) -> Option<&str> {
    let index = usize::try_from(result.checked_sub(BUTTON_BASE)?).ok()?;
    model.buttons.get(index).map(|(id, _)| id.as_str())
}

#[cfg(windows)]
pub use imp::{show, spawn_detached};

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt as _;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use anyhow::{Context as _, Result};
    use windows::Win32::Foundation::{
        HANDLE_FLAG_INHERIT, HANDLE_FLAGS, HWND, LPARAM, S_OK, SetHandleInformation, WPARAM,
    };
    use windows::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Controls::{
        TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0,
        TD_INFORMATION_ICON, TD_WARNING_ICON, TDCBF_CLOSE_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION,
        TDF_USE_COMMAND_LINKS, TDN_CREATED, TaskDialogIndirect,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FLASHW_ALL, FLASHW_TIMERNOFG, FLASHWINFO, FlashWindowEx, HWND_TOPMOST, ICON_BIG,
        ICON_SMALL, LoadIconW, SWP_NOMOVE, SWP_NOSIZE, SendMessageW, SetForegroundWindow,
        SetWindowPos, WM_SETICON,
    };
    use windows::core::{HSTRING, PCWSTR};

    use super::{BUTTON_BASE, Icon, Model, chosen};

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    /// Show the dialog and wait for a choice. Returns the chosen button id, `None` if closed.
    pub fn show(model: &Model) -> Result<Option<String>> {
        let title = HSTRING::from(model.window_title.as_str());
        let instruction = HSTRING::from(model.instruction.as_str());
        let content = model.content.as_deref().map(HSTRING::from);
        let labels: Vec<HSTRING> = model
            .buttons
            .iter()
            .map(|(_, l)| HSTRING::from(l.as_str()))
            .collect();
        let buttons: Vec<TASKDIALOG_BUTTON> = labels
            .iter()
            .zip(BUTTON_BASE..)
            .map(|(l, id)| TASKDIALOG_BUTTON {
                nButtonID: id,
                pszButtonText: PCWSTR(l.as_ptr()),
            })
            .collect();

        let mut flags = TDF_ALLOW_DIALOG_CANCELLATION;
        if model.command_links {
            flags |= TDF_USE_COMMAND_LINKS;
        }
        let config = TASKDIALOGCONFIG {
            cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
            dwFlags: flags,
            // without custom buttons, offer a plain Close
            dwCommonButtons: if buttons.is_empty() {
                TDCBF_CLOSE_BUTTON
            } else {
                Default::default()
            },
            pszWindowTitle: PCWSTR(title.as_ptr()),
            Anonymous1: TASKDIALOGCONFIG_0 {
                pszMainIcon: match model.icon {
                    Icon::Information => TD_INFORMATION_ICON,
                    Icon::Warning => TD_WARNING_ICON,
                },
            },
            pszMainInstruction: PCWSTR(instruction.as_ptr()),
            pszContent: content
                .as_ref()
                .map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
            cButtons: buttons.len() as u32,
            pButtons: if buttons.is_empty() {
                std::ptr::null()
            } else {
                buttons.as_ptr()
            },
            pfCallback: Some(on_notify),
            ..Default::default()
        };
        let mut result = 0i32;
        // SAFETY: config and every string/button it points to outlive the call.
        unsafe { TaskDialogIndirect(&config, Some(&mut result), None, None) }
            .context("TaskDialogIndirect")?;
        Ok(chosen(model, result).map(str::to_string))
    }

    /// Bring the new dialog to the user's attention: it may have been started in the background.
    unsafe extern "system" fn on_notify(
        hwnd: HWND,
        msg: TASKDIALOG_NOTIFICATIONS,
        _w: WPARAM,
        _l: LPARAM,
        _data: isize,
    ) -> windows::core::HRESULT {
        if msg == TDN_CREATED {
            // SAFETY: hwnd is the dialog window TaskDialogIndirect hands us; the icon is resource 1
            // of this exe (res/toastback.rc).
            unsafe {
                if let Ok(module) = GetModuleHandleW(PCWSTR::null())
                    && let Ok(icon) = LoadIconW(Some(module.into()), PCWSTR(1 as _))
                {
                    let lparam = Some(LPARAM(icon.0 as isize));
                    SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), lparam);
                    SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), lparam);
                }
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE,
                );
                if !SetForegroundWindow(hwnd).as_bool() {
                    // no foreground rights (not started by a click): flash the taskbar button
                    let info = FLASHWINFO {
                        cbSize: size_of::<FLASHWINFO>() as u32,
                        hwnd,
                        dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
                        uCount: 0,
                        dwTimeout: 0,
                    };
                    let _ = FlashWindowEx(&info);
                }
            }
        }
        S_OK
    }

    /// Start `exe args…` fully detached, so the caller (and WSL interop waiting on it) returns now.
    pub fn spawn_detached(exe: &Path, args: &[&str]) -> Result<()> {
        // Our std handles may be WSL interop's pipes. If the child inherited them, the WSL side
        // would wait until the dialog closes. Stop them from being inherited.
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: plain handle queries/updates on this process's own std handles.
            unsafe {
                if let Ok(h) = GetStdHandle(which)
                    && !h.is_invalid()
                    && !h.0.is_null()
                {
                    let _ = SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
                }
            }
        }
        let spawn = |extra: u32| {
            Command::new(exe)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | extra)
                .spawn()
        };
        // leave the caller's job object if it allows that, so closing it doesn't kill the dialog
        spawn(CREATE_BREAKAWAY_FROM_JOB)
            .or_else(|_| spawn(0))
            .with_context(|| format!("start {}", exe.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_spec_to_dialog() {
        let s = Spec::parse(
            r#"{"title": "Reboot?", "body": "Updates are installed.", "details": "Takes 2 minutes.",
                "scenario": "alarm",
                "buttons": [{"id": "now", "label": "Reboot now"}, {"id": "later", "label": "Later"}]}"#,
        )
        .unwrap();
        let m = model(&s);
        assert_eq!(m.window_title, "Reboot?");
        assert_eq!(m.instruction, "Updates are installed.");
        assert_eq!(m.content.as_deref(), Some("Takes 2 minutes."));
        assert_eq!(m.icon, Icon::Warning);
        assert!(!m.command_links);
        assert_eq!(chosen(&m, BUTTON_BASE), Some("now"));
        assert_eq!(chosen(&m, BUTTON_BASE + 1), Some("later"));
        assert_eq!(chosen(&m, BUTTON_BASE + 2), None);
        assert_eq!(chosen(&m, 2), None); // IDCANCEL
        assert_eq!(chosen(&m, 8), None); // IDCLOSE
    }

    #[test]
    fn title_only_and_command_links() {
        let s = Spec::parse(
            r#"{"title": "Pick one", "buttons": [
                {"id": "a", "label": "A"}, {"id": "b", "label": "B"}, {"id": "c", "label": "C"}]}"#,
        )
        .unwrap();
        let m = model(&s);
        assert_eq!(m.instruction, "Pick one");
        assert_eq!(m.content, None);
        assert_eq!(m.icon, Icon::Information);
        assert!(m.command_links);
    }
}
