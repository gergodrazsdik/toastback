# toastback

Async Windows toast notifications with callbacks, meant to be driven from WSL and scripts. Rust, built in WSL for `x86_64-pc-windows-msvc` with cargo-xwin. Public repo: https://github.com/gergodrazsdik/toastback (GPL-3.0-or-later).

The plan is `PLAN.md`: §2 holds the locked decisions, §10 the milestones, §13 the working agreement. Follow it. Spike results and later decisions go in `docs/decisions/`.

## Commands

- `just build` / `just run <args>`: Windows release build of both exes via `cargo xwin` (default target in `.cargo/config.toml`, static CRT).
- `just check` (alias `lint`): `cargo fmt --check`, clippy for the Windows target with `-D warnings`, shellcheck on `scripts/*.sh`. Run before calling work done.
- `just test`: host-target tests (`cargo test --target x86_64-unknown-linux-gnu`), then the Windows tests through WSL interop (cargo-xwin forces `wine` as the runner, so the recipe overrides it), then `validate` on every example.
- `just install`: build and install for this distro (the dev loop); `toastback doctor` checks the result.
- The built `.exe` runs from WSL directly (`target/x86_64-pc-windows-msvc/release/toastback.exe`). It's GUI-subsystem: redirected output arrives as pipes, and in a terminal `console.rs` attaches the missing streams to the parent console.
- `scripts/make-icon.sh` re-renders `res/icon.png` and `res/icon.ico` from `res/icon.svg` (placeholder icon).

## Conventions

- Layout: a library (`src/lib.rs`, entry `toastback::main()`) with two binaries: `toastback.exe` (`src/main.rs`, GUI subsystem: protocol handler, dialogs, WSL shim target) and `toastback-cli.exe` (`src/bin/toastback-cli.rs`, console subsystem, for cmd/PowerShell). See `docs/decisions/003-m3.md`.
- Every source file starts with `// SPDX-License-Identifier: GPL-3.0-or-later`.
- Keep Windows-specific code behind `#[cfg(windows)]`; platform-neutral logic (spec, validation, URI parsing, path conversion) stays testable on the Linux host.
- `unsafe` only in `toast.rs`, `dialog.rs`, `exec.rs`, `install.rs`, `console.rs`, `registry.rs`, wrapped in small safe functions with `// SAFETY:` comments.
- Boring, explicit code; no async runtime.
- Crates: only those in PLAN.md §7 (`windows`, `serde`, `serde_json`, `anyhow`, `clap`, `getrandom`, `embed-resource`). Ask before adding others; add with `cargo add`, and add `windows` features as needed.
- Ask before changing a locked decision (§2). Never run `just bump`/`just release` or push tags unasked.
- `main` is protected (ruleset): every change goes through a PR on a branch, merged by squash once CI is green; never
  push to main. Commit on a branch, push it, `gh pr create`, wait for the checks, then `gh pr merge --squash` when the
  user agrees.
- Keep `CHANGELOG.md` current ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/)): every user-visible change
  adds a line under `[Unreleased]` in the same PR. Versions follow semver; release with `just bump X.Y.Z` (PR), then
  `just release X.Y.Z` after it's merged. See `docs/decisions/005-github.md`.
- New spec fields or commands: update README.md, add or extend an example in `examples/` (registered in `src/examples.rs` as built-in or repo-only), and add a step to `docs/manual-test.md`.
- After each milestone: `just check && just test && just install`, then give the user the manual steps for the acceptance criteria.
