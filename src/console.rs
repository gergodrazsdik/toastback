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
    AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    SetStdHandle,
};

const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

fn missing(which: STD_HANDLE) -> bool {
    // SAFETY: plain query of this process's std handle.
    unsafe { GetStdHandle(which) }.map_or(true, |h| h.is_invalid() || h.0.is_null())
}

/// Give streams that have no handle the parent's console. Does nothing without a parent console.
pub fn attach_missing_streams() {
    let streams = [
        (STD_INPUT_HANDLE, "CONIN$"),
        (STD_OUTPUT_HANDLE, "CONOUT$"),
        (STD_ERROR_HANDLE, "CONOUT$"),
    ];
    let todo: Vec<_> = streams.into_iter().filter(|(w, _)| missing(*w)).collect();
    // SAFETY: AttachConsole takes no pointers; it fails harmlessly without a parent console.
    if todo.is_empty() || unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err() {
        return;
    }
    for (which, device) in todo {
        let Ok(file) = OpenOptions::new().read(true).write(true).open(device) else {
            continue;
        };
        // the handle lives for the rest of the process, as a std handle should
        let raw = file.into_raw_handle();
        // SAFETY: raw is a valid console handle we own and never close.
        let _ = unsafe { SetStdHandle(which, HANDLE(raw)) };
    }
}
