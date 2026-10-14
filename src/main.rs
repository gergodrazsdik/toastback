// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg_attr(windows, windows_subsystem = "windows")]

use clap::{Parser, Subcommand};

#[cfg(windows)]
mod spike;

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TOASTBACK_GIT_SHA"),
    ")"
);

/// Async Windows toasts with callbacks, for WSL and scripts.
#[derive(Parser)]
#[command(version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Protocol handler for toastback:// URIs (internal)
    #[command(hide = true)]
    Activate { uri: String },
    /// M0 spikes (temporary)
    #[command(hide = true)]
    Spike {
        #[arg(value_parser = ["stdio", "register", "unregister", "toast", "dialog"])]
        which: String,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::ExitCode::from(code),
        Err(e) => {
            eprintln!("toastback: {e:#}");
            std::process::ExitCode::from(1)
        }
    }
}

#[cfg(windows)]
fn run(cli: Cli) -> anyhow::Result<u8> {
    match cli.command {
        Command::Activate { uri } => spike::activate(&uri).map(|()| 0),
        Command::Spike { which } => spike::run(&which),
    }
}

#[cfg(not(windows))]
fn run(_cli: Cli) -> anyhow::Result<u8> {
    anyhow::bail!("toastback only runs on Windows")
}
