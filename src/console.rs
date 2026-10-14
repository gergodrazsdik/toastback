// SPDX-License-Identifier: GPL-3.0-or-later
//! Terminal output for a GUI-subsystem exe.
//!
//! Started from an interactive terminal (WSL or PowerShell), the exe gets no std handles at all, so
//! everything it prints is lost. Redirected streams (pipes, files) arrive as normal handles. So: for
//! each stream without a handle, attach to the parent's console and point the stream at it.
//! See docs/decisions/000-spikes.md.

use std::fs::OpenOptions;
use std::os::windows::io::IntoRawHandle as _;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    AttachConsole, CONSOLE_MODE, DISABLE_NEWLINE_AUTO_RETURN, GetConsoleMode, GetStdHandle,
    STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
    SetStdHandle,
};

const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

fn missing(which: STD_HANDLE) -> bool {
    // SAFETY: plain query of this process's std handle.
    unsafe { GetStdHandle(which) }.map_or(true, |h| h.is_invalid() || h.0.is_null())
}

/// Restores the console's output mode when dropped.
pub struct ModeGuard {
    handle: HANDLE,
    mode: CONSOLE_MODE,
}

impl Drop for ModeGuard {
    fn drop(&mut self) {
        // SAFETY: the handle stays open for the whole process.
        let _ = unsafe { SetConsoleMode(self.handle, self.mode) };
    }
}

/// Give streams that have no handle the parent's console. Does nothing without a parent console.
///
/// The console WSL relays to has `DISABLE_NEWLINE_AUTO_RETURN` set, so a bare `\n` doesn't return to
/// column 0. That flag is cleared while we write; the returned guard puts it back. The mode belongs
/// to the shared screen buffer, so it must be restored before exiting.
pub fn attach_missing_streams() -> Option<ModeGuard> {
    let streams = [
        (STD_INPUT_HANDLE, "CONIN$"),
        (STD_OUTPUT_HANDLE, "CONOUT$"),
        (STD_ERROR_HANDLE, "CONOUT$"),
    ];
    let todo: Vec<_> = streams.into_iter().filter(|(w, _)| missing(*w)).collect();
    // SAFETY: AttachConsole takes no pointers; it fails harmlessly without a parent console.
    if todo.is_empty() || unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err() {
        return None;
    }
    let mut guard = None;
    for (which, device) in todo {
        let Ok(file) = OpenOptions::new().read(true).write(true).open(device) else {
            continue;
        };
        // the handle lives for the rest of the process, as a std handle should
        let handle = HANDLE(file.into_raw_handle());
        // SAFETY: handle is a valid console handle we own and never close.
        let _ = unsafe { SetStdHandle(which, handle) };
        if device == "CONOUT$" && guard.is_none() {
            let mut mode = CONSOLE_MODE::default();
            // SAFETY: valid console handle, out-pointer to a local.
            if unsafe { GetConsoleMode(handle, &mut mode) }.is_ok()
                && mode.contains(DISABLE_NEWLINE_AUTO_RETURN)
            {
                // SAFETY: valid console handle.
                let _ = unsafe { SetConsoleMode(handle, mode & !DISABLE_NEWLINE_AUTO_RETURN) };
                guard = Some(ModeGuard { handle, mode });
            }
        }
    }
    guard
}
