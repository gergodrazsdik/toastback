# toastback: build Windows binaries from WSL with cargo-xwin.

target := "x86_64-pc-windows-msvc"
exe := "target/" + target + "/release/toastback.exe"

# List recipes
[private]
default:
    @just --list --unsorted

# Build the Windows release binary
[group('dev')]
build:
    cargo xwin build --release --target {{ target }}

# Type-check for Windows (fast, no linking)
[group('dev')]
check:
    cargo xwin check --target {{ target }}

# Build and run the Windows binary from WSL (args go to the exe)
[group('dev')]
run *args: build
    {{ exe }} {{ args }}

# Unit tests (host target; Windows-only code needs `cargo xwin test --no-run`)
[group('dev')]
test:
    cargo test

# Format check + clippy for the Windows target, warnings as errors
[group('dev')]
lint:
    cargo fmt --check
    cargo xwin clippy --target {{ target }} --all-targets -- -D warnings

# Format the code
[group('dev')]
fmt:
    cargo fmt
