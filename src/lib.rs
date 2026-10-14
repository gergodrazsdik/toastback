// SPDX-License-Identifier: GPL-3.0-or-later
//! toastback: async Windows toasts with callbacks, for WSL and scripts.
//!
//! Two binaries share this crate: `toastback.exe` (GUI subsystem: no console window when Windows
//! starts it for a click) and `toastback-cli.exe` (console subsystem, for cmd/PowerShell, which
//! don't wait for GUI programs).

// Most logic is platform-neutral and unit-tested on the Linux host, where only the tests use it.
#![cfg_attr(not(windows), allow(dead_code))]

mod activate;
mod cli;
#[cfg(windows)]
mod console;
mod context;
mod dialog;
mod doctor;
mod exec;
mod install;
#[cfg(windows)]
mod registry;
mod show;
mod spec;
mod store;
mod toast;

use std::process::ExitCode;

use crate::cli::Cli;

pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TOASTBACK_GIT_SHA"),
    ")"
);

/// The entry point of both binaries.
pub fn main() -> ExitCode {
    // The protocol handler and the dialog process have no terminal; anything else may have one.
    // The guard restores the console mode on drop, so every path below must return, not exit().
    #[cfg(windows)]
    let _console = std::env::args_os()
        .nth(1)
        .is_none_or(|a| a != "activate" && a != "__dialog")
        .then(console::prepare)
        .flatten();
    let cli = match Cli::parse_with_default(std::env::args_os()) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(e.exit_code() as u8);
        }
    };
    match cli::run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("toastback: {e:#}");
            ExitCode::from(1)
        }
    }
}
