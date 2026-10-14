// SPDX-License-Identifier: GPL-3.0-or-later
//! toastback: async Windows toasts with callbacks, for WSL and scripts.
//!
//! Two binaries share this crate: `toastback.exe` (GUI subsystem: no console window when Windows
//! starts it for a click) and `toastback-cli.exe` (console subsystem, for cmd/PowerShell, which
//! don't wait for GUI programs).

// Most logic is platform-neutral and unit-tested on the Linux host, where only the tests use it.
#![cfg_attr(not(windows), allow(dead_code))]

/// `print!` and `println!` that don't panic: a reader that went away (`toastback log | head`) is
/// fine, and with `panic = "abort"` a panic would also skip restoring the console. Other write
/// errors (a full disk behind `> file`) make the exit code 1, see [`main`].
#[allow(unused_macros)] // only Windows code prints
macro_rules! out {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        $crate::note_stdout(write!(std::io::stdout().lock(), $($arg)*));
    }};
}

#[allow(unused_macros)] // only Windows code prints
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        $crate::note_stdout(writeln!(std::io::stdout().lock(), $($arg)*));
    }};
}

static STDOUT_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Remember a failed stdout write, unless the reader just went away.
#[allow(dead_code)] // only Windows code prints
fn note_stdout(r: std::io::Result<()>) {
    if let Err(e) = r
        && e.kind() != std::io::ErrorKind::BrokenPipe
        && !STDOUT_FAILED.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        eprintln!("toastback: writing the output failed: {e}");
    }
}

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
        Ok(_) if STDOUT_FAILED.load(std::sync::atomic::Ordering::Relaxed) => ExitCode::from(1),
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("toastback: {e:#}");
            ExitCode::from(1)
        }
    }
}
