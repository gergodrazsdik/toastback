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

# Format check, clippy for the Windows target (warnings as errors), shellcheck, yamllint, actionlint
[group('dev')]
check:
    cargo fmt --check
    cargo xwin clippy --release --target {{ target }} --all-targets -- -D warnings
    shellcheck scripts/*.sh examples/*.sh
    shellcheck -s bash res/shim.sh.tmpl
    yamllint -s .github .yamllint
    actionlint

alias lint := check

# Unit tests: platform-neutral code on the host, then the Windows build through WSL interop
[group('dev')]
test:
    cargo test --target {{ host }}
    CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=/usr/bin/env cargo xwin test --target {{ target }}
    cargo xwin build --target {{ target }}
    target/{{ target }}/debug/toastback.exe validate examples/*.json

# Format the code
[group('dev')]
fmt:
    cargo fmt

# Build and install for this distro (the normal dev loop)
[group('dev')]
install: build
    {{ exe }} install --wsl "$WSL_DISTRO_NAME" --distro "$WSL_DISTRO_NAME" --user "$(id -un)"

# Remove the installation (keeps logs; `toastback uninstall --purge` removes those too)
[group('dev')]
uninstall:
    toastback uninstall

# Show every example notification
[group('try')]
demo:
    for f in examples/*.json; do toastback "$f"; sleep 1; done

# Recent toastback events
[group('try')]
log n="20":
    toastback log -n {{ n }}

# Bump the version, commit, tag and push; the release workflow builds and publishes it
[group('release')]
[confirm('Commit, tag and push this release to GitHub? [y/N]')]
release version:
    test -z "$(git status --porcelain)" || { echo "working tree not clean"; exit 1; }
    [[ "{{ version }}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must look like 1.2.3"; exit 1; }
    sed -i '/^\[package\]/,/^\[/ s/^version = ".*"/version = "{{ version }}"/' Cargo.toml
    grep -q '^version = "{{ version }}"$' Cargo.toml || { echo "version not updated in Cargo.toml"; exit 1; }
    cargo xwin build --release --target {{ target }}  # refreshes Cargo.lock
    git commit -am "release v{{ version }}"
    git tag -a "v{{ version }}" -m "v{{ version }}"
    git push --follow-tags
    sleep 10
    gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
