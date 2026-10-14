# toastback

Async Windows toast notifications with callbacks, meant to be driven from WSL and scripts. Rust, built in WSL for `x86_64-pc-windows-msvc` with cargo-xwin. Public repo: https://github.com/gergodrazsdik/toastback (GPL-3.0-or-later).

## Commands

- `just build` / `just check` / `just run <args>`: Windows build via `cargo xwin` (target pinned in `rust-toolchain.toml`).
- `just lint`: `cargo fmt --check` and clippy for the Windows target with `-D warnings`. Run before calling work done.
- `just test`: host-target unit tests. Code behind `#[cfg(windows)]` only compiles for the Windows target: check it with `just check`/`just lint`, and smoke-test the `.exe` by running it from WSL (Windows interop).

## Conventions

- Keep Windows-specific code behind `#[cfg(windows)]`; platform-neutral logic (argument parsing, callback plumbing) stays testable on the Linux host.
- New dependencies with `cargo add`; prefer well-maintained crates (`windows`/`windows-sys` for WinRT/Win32).
- The plan for the project lives in the plan file the user provides; follow it.
