// SPDX-License-Identifier: GPL-3.0-or-later
//! `toastback.exe`: GUI subsystem, so Windows never opens a console window for it.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    toastback::main()
}
