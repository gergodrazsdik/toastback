// SPDX-License-Identifier: GPL-3.0-or-later
//! `toastback-cli.exe`: console subsystem, for interactive cmd/PowerShell. They wait for console
//! programs (output before the next prompt, `$LASTEXITCODE` set) but not for GUI ones.

fn main() -> std::process::ExitCode {
    toastback::main()
}
