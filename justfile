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

# Check the installation (registration, notifications, WSL shim)
[group('try')]
doctor:
    toastback doctor

# Step 1 of a release: on a new branch, bump Cargo.toml, date the changelog, push and open a PR
[group('release')]
[confirm('Create branch release-v{{ version }}, push it and open a PR? [y/N]')]
bump version:
    test -z "$(git status --porcelain --untracked-files=no)" || { echo "working tree not clean"; exit 1; }
    git switch main
    git pull --ff-only
    git switch -c "release-v{{ version }}"
    scripts/bump.sh "{{ version }}"
    git commit -am "Release v{{ version }}"
    git push -u origin "release-v{{ version }}"
    gh pr create --title "Release v{{ version }}" --body "Bumps the version and dates the changelog. After merging: just release {{ version }}"

# Step 2, after the bump PR is merged: tag main's head and push the tag; the release workflow publishes it
[group('release')]
[confirm('Tag main as v{{ version }} and push the tag? This publishes the release. [y/N]')]
release version:
    test -z "$(git status --porcelain --untracked-files=no)" || { echo "working tree not clean"; exit 1; }
    git switch main
    git pull --ff-only
    grep -q '^version = "{{ version }}"$' Cargo.toml || { echo "main isn't at {{ version }}: merge the 'just bump' PR first"; exit 1; }
    grep -q '^## \[{{ version }}\]' CHANGELOG.md || { echo "CHANGELOG.md has no [{{ version }}] section"; exit 1; }
    git tag -a "v{{ version }}" -m "v{{ version }}"
    git push origin "v{{ version }}"
    sleep 15
    gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')" || echo "watch failed: see https://github.com/gergodrazsdik/toastback/actions"
