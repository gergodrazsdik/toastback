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

# Build and run the Windows binary from WSL (args go to the exe, as they are)
[group('dev')]
[positional-arguments]
run *args: build
    {{ exe }} "$@"

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
[confirm('Create the release branch for this version, push it and open a PR? [y/N]')]
[positional-arguments]
bump version:
    #!/usr/bin/env bash
    set -euo pipefail
    test -z "$(git status --porcelain --untracked-files=no)" || { echo "working tree not clean"; exit 1; }
    git switch main
    git pull --ff-only
    scripts/bump.sh --check "$1" # before the branch exists, so a bad version leaves nothing behind
    git switch -c "release-v$1"
    scripts/bump.sh "$1"
    git commit -am "Release v$1"
    git push -u origin "release-v$1"
    gh pr create --title "Release v$1" --body "Bumps the version and dates the changelog. After merging: just release $1"

# Step 2, after the bump PR is merged: tag the bump commit on main and push the tag; the release workflow publishes it
[group('release')]
[confirm('Tag the release commit on main and push the tag? This publishes the release. [y/N]')]
[positional-arguments]
release version:
    #!/usr/bin/env bash
    set -euo pipefail
    v=$1
    [[ $v =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage: just release X.Y.Z"; exit 1; }
    test -z "$(git status --porcelain --untracked-files=no)" || { echo "working tree not clean"; exit 1; }
    git switch main
    git pull --ff-only
    grep -qxF "version = \"$v\"" Cargo.toml || { echo "main isn't at $v: merge the 'just bump' PR first"; exit 1; }
    grep -qF "## [$v] - " CHANGELOG.md || { echo "CHANGELOG.md has no [$v] section"; exit 1; }
    # The commit that set the version, not main's head: PRs merged after it wait for the next release.
    # Walk back through the commits that changed Cargo.toml while its package version is still $v.
    pkg_version() { git show "$1:Cargo.toml" | sed -n '/^\[package\]/,/^\[/ s/^version = "\(.*\)"/\1/p'; }
    commit=
    while read -r c; do
        [ "$(pkg_version "$c")" = "$v" ] || break
        commit=$c
    done < <(git log --format=%H -- Cargo.toml)
    [ -n "$commit" ] || { echo "no commit sets version $v in Cargo.toml"; exit 1; }
    if [ "$commit" != "$(git rev-parse HEAD)" ]; then
        echo "tagging $(git log -1 --format='%h %s' "$commit");"
        echo "the $(git rev-list --count "$commit..HEAD") commit(s) merged after it go into the next release"
    fi
    git tag -a "v$v" -m "v$v" "$commit"
    git push origin "v$v"
    # the run for this tag; it can take a while to be queued
    run=
    for _ in $(seq 24); do
        run=$(gh run list --workflow release.yml --branch "v$v" --event push --limit 1 --json databaseId -q '.[0].databaseId') || run=
        [ -n "$run" ] && break
        sleep 5
    done
    [ -n "$run" ] || { echo "no release run for v$v yet: see https://github.com/gergodrazsdik/toastback/actions"; exit 1; }
    gh run watch "$run" --exit-status
