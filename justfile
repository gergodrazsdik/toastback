# toastback: build Windows binaries from WSL with cargo-xwin.

set shell := ["bash", "-euo", "pipefail", "-c"]

target := "x86_64-pc-windows-msvc"
host := "x86_64-unknown-linux-gnu"
exe := "target/" + target + "/release/toastback.exe"

# List recipes
[private]
default:
    @just --list --unsorted

# One-time setup: target + pre-download the MSVC CRT/SDK (accepts Microsoft's license)
[group('dev')]
setup:
    rustup target add {{ target }}
    command -v cargo-xwin >/dev/null || { echo "install cargo-xwin first: cargo install --locked cargo-xwin"; exit 1; }
    cargo xwin cache xwin

# Build the Windows release binary
[group('dev')]
build:
    cargo xwin build --release --target {{ target }}

# Build and run the Windows binary from WSL (args go to the exe)
[group('dev')]
run *args: build
    {{ exe }} {{ args }}

# Format check, clippy for the Windows target (warnings as errors), shellcheck
[group('dev')]
check:
    cargo fmt --check
    cargo xwin clippy --release --target {{ target }} --all-targets -- -D warnings
    shellcheck scripts/*.sh

alias lint := check

# Unit tests: platform-neutral code on the host, then the Windows build through WSL interop
[group('dev')]
test:
    cargo test --target {{ host }}
    CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=/usr/bin/env cargo xwin test --target {{ target }}

# Format the code
[group('dev')]
fmt:
    cargo fmt
