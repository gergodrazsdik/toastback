# Agent instructions: toastback

These instructions are for coding agents (Claude Code, Codex and others) working in this repo. Humans: see
[CONTRIBUTING.md](CONTRIBUTING.md).

## What toastback is

Async Windows toast notifications with callbacks, driven from WSL and scripts. One self-contained Windows exe,
written in Rust and built in WSL for `x86_64-pc-windows-msvc` with cargo-xwin. It shows a toast or a native dialog
and returns at once; when someone clicks a button, Windows starts toastback again through the `toastback://`
protocol, and it runs the command attached to that button, in WSL or on Windows. Public repo, GPL-3.0-or-later,
released since 1.0.0: the spec format, the CLI and the exit codes are a public API.

Where the truth lives:

- [docs/plan.md](docs/plan.md): the original plan. §2 holds the locked decisions, §4 the spec format, §7 the allowed
  crates.
- [docs/adr/](docs/adr/README.md): architecture decision records (ADRs), made since the plan, which refine or
  supersede it.
- [CHANGELOG.md](CHANGELOG.md): what changed in each release, and what's waiting under `[Unreleased]`.
- [README.md](README.md): the user documentation (install, usage, the spec, limitations, troubleshooting).
- [docs/manual-test.md](docs/manual-test.md): the checks that need a person at a Windows desktop.

If the plan conflicts with a decision record, these instructions or a request, stop and ask the maintainer; don't
pick a side yourself.

## Layout

```text
AGENTS.md CLAUDE.md README.md LICENSE CHANGELOG.md
CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md SUPPORT.md
Cargo.toml Cargo.lock rust-toolchain.toml build.rs .cargo/config.toml
justfile  local.just.example  lefthook.yml  .editorconfig  lint configs
src/                 lib.rs (toastback::main()), main.rs (toastback.exe), bin/toastback-cli.rs, one module per concern
skills/toastback/    SKILL.md, the agent skill: embedded by src/skill.rs (`toastback skill`, `install --skill`)
res/                 the Windows resources (manifest, icon, version info), status icons, the WSL shim and notify-send wrapper templates
examples/            example specs; src/examples.rs registers them as built-in or repo-only
scripts/             install.sh (release asset), bump.sh, package.sh, winget.sh, scoop.sh, make-icon.sh, with-tool.sh,
                     check-spdx.sh, install-ci-tools.sh, tool-updates.sh, github-settings.sh
packaging/winget/    the winget manifest templates
packaging/scoop/     the Scoop manifest template (the bucket is gergodrazsdik/scoop-bucket)
docs/plan.md         the original plan
docs/adr/            architecture decision records
docs/manual-test.md  the manual test checklist
docs/dev-setup.md    prerequisites and local setup
.claude/settings.json  committed Claude Code sandbox and deny rules
.github/             workflows/ (ci.yml, lint.yml, release.yml), rulesets/, issue and PR templates, CODEOWNERS,
                     dependabot.yml
```

Keep this tree current when you add top-level files or folders.

## Working agreement

- Work on a branch (`feat/…`, `fix/…`, `docs/…`, `ci/…`, `chore/…`). **Never commit or push to `main`**: it's
  protected, and every change goes through a pull request with green CI.
- Push the branch and open the pull request with `gh pr create`. The title is a short imperative sentence ("Fix
  callbacks and notification text"); the squash merge uses the title and description as the commit on `main`, so
  keep both clean.
- **Merging is the maintainer's call.** Once CI is green, ask; merge with `gh pr merge --squash` only when they
  agree. Never enable auto-merge.
- **Never run `just bump` or `just release`, and never create, move or delete a tag**, unless the maintainer asks.
  Never run `just winget-submit` or `just winget-update` unasked: they open public pull requests on
  `microsoft/winget-pkgs`.
- Write commit messages to a file outside the repo and commit with `git commit -F <file>`. Commits are signed when
  your git config signs; keep it on.
- Every source file starts with `// SPDX-License-Identifier: GPL-3.0-or-later` (`#` in shell scripts, after the
  shebang).
- Never bypass a hook (`--no-verify`), never weaken or skip a check, and never relax a lint rule to get a change
  through. Fix the cause, or raise it.
- Ask before any GitHub write other than pushing your branch and creating or editing your pull requests: comments,
  reviews, closing, labels, settings, releases, force pushes outside your own stack.
- Every user-visible change adds a line under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md) in the same pull request
  ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/): Added, Changed, Deprecated, Removed, Fixed, Security).
- A new spec field or command updates README.md, adds or extends an example in `examples/` (registered in
  `src/examples.rs` as built-in or repo-only), and adds a step to `docs/manual-test.md`.
- A change to install, the winget or Scoop manifest or packaging runs `just winget-test` and `just scoop-test` before
  it's merged.
- After a larger change: `just check && just test && just install`, then give the maintainer the manual steps from
  `docs/manual-test.md` that cover it.

## Stacked PRs

When one change builds on another that isn't merged yet, open the pull requests as a native GitHub stack with the
`gh stack` extension ([about stacked PRs](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs),
[commands](https://docs.github.com/en/pull-requests/reference/stacked-prs-cli-commands)), not as pull requests based
on each other's branches. GitHub then retargets the rest of the stack when one merges.

- Create the branches with `gh stack init <branch>…` (or `gh stack add <branch>` on the top branch), and commit to
  each as usual.
- Publish with `gh stack submit --auto --open` (without `--auto` it opens an interactive editor), then give each pull
  request its real title and body with `gh pr edit --title … --body-file <file>`.
- The maintainer merges the stack bottom-up. Afterwards, `gh stack sync --prune` updates `main` and the remaining
  branches locally and deletes the merged ones.
- `sync` and `rebase` may force-push the stack's own branches (with lease). `link`, `merge` and `unstack` write to
  GitHub: ask first. Never run `gh stack merge`.

## Commands

Use `just` (`just --list`):

| Recipe | What it does |
|---|---|
| `just setup` | One-time: the Windows target and the MSVC CRT and SDK for cargo-xwin. |
| `just build` | Windows release build of both exes (`cargo xwin`). |
| `just run <args>` | Build, then run `toastback.exe` from WSL with the arguments as they are. |
| `just check` | `cargo fmt --check`, clippy for the Windows target with `-D warnings`, then `just lint`. |
| `just lint` | Every linter the `lint` workflow runs: Markdown, YAML, actionlint, zizmor, pinact, shellcheck, typos, EditorConfig, gitleaks, the SPDX identifiers. |
| `just pin` | Pin workflow actions to full commit SHAs (pinact). |
| `just tool-updates` | Which linter versions pinned outside Dependabot's reach have newer releases, and when each is due (7-day cooldown). Read-only. |
| `just test` | Host-target tests, the Windows tests through WSL interop, then `validate` on every example. |
| `just fmt` | Format the code. |
| `just tools` / `just hooks` | Check the development tools, with install hints / install the lefthook git hooks. |
| `just install` / `just uninstall` | Build and install for this distro and on the Windows PATH (the dev loop) / remove it. |
| `just demo` / `just log` / `just doctor` | Show every example / recent events / check the installation. |
| `just winget-setup`, `winget-manifest X.Y.Z`, `winget-test`, `winget-test-release X.Y.Z` | winget packaging and its local tests ([ADR 0013](docs/adr/0013-winget.md)). |
| `just winget-submit X.Y.Z` / `just winget-update X.Y.Z` | Maintainer only: public pull requests on `microsoft/winget-pkgs`. |
| `just scoop-setup`, `scoop-manifest X.Y.Z`, `scoop-test` | Scoop packaging and its local test ([ADR 0020](docs/adr/0020-scoop.md)). The bucket's bot updates the published manifest. |
| `just github-settings <repo>` / `just github-settings-check <repo>` | Maintainer only: apply the repository settings and rulesets ([ADR 0016](docs/adr/0016-settings-as-code.md)) / read them back and report differences. |
| `just bump X.Y.Z` / `just release X.Y.Z` | Maintainer only: the release flow ([ADR 0012](docs/adr/0012-repository-and-release.md)). |

Run `just check` and `just test` before every commit that changes code (`just lint` alone for docs). The hooks run
rustfmt and the linters on staged files, plus gitleaks. A missing linter is skipped with a message locally; CI
requires them all.

The built exe runs from WSL directly (`target/x86_64-pc-windows-msvc/release/toastback.exe`). It's GUI-subsystem:
redirected output arrives as pipes, and in a terminal `console.rs` attaches the missing streams to the parent
console. `scripts/make-icon.sh` re-renders `res/icon.png` and `res/icon.ico` from `res/icon.svg`.

## Code conventions

- A library (`src/lib.rs`, entry `toastback::main()`) with two binaries: `toastback.exe` (`src/main.rs`, GUI
  subsystem: protocol handler, dialogs, the WSL shim's target) and `toastback-cli.exe` (`src/bin/toastback-cli.rs`,
  console subsystem, for cmd and PowerShell). See [ADR 0003](docs/adr/0003-terminal-output.md).
- Windows-specific code stays behind `#[cfg(windows)]`. Platform-neutral logic (the spec, validation, URI parsing,
  path conversion) stays testable on the Linux host.
- `unsafe` only in `toast.rs`, `dialog.rs`, `exec.rs`, `install.rs`, `console.rs` and `registry.rs`, wrapped in small
  safe functions with `// SAFETY:` comments.
- Boring, explicit code; no async runtime.
- Crates: only those in [the plan's §7](docs/plan.md#7-project-setup-wsl) (`windows`, `serde`, `serde_json`,
  `anyhow`, `clap`, `getrandom`, `embed-resource`). Ask before adding another; add it with `cargo add`, and add
  `windows` features as needed.
- Nothing machine-specific in committed files: no absolute paths, hostnames, usernames, distro names,
  secret-manager entries or credentials.

## Decisions (ADRs)

A decision, or a finding that changes the implementation, is recorded as an ADR in [docs/adr/](docs/adr/README.md),
in the pull request that makes it, using the template there, and added to its index. Accepted ADRs aren't rewritten
to change their decision; a new one amends or supersedes them. The plan's §2 decisions are locked
([ADR 0001](docs/adr/0001-baseline.md)): a choice that changes one of them, an accepted ADR or the public API needs
the maintainer's approval first.

## When something is unknown

Don't guess on scope, behaviour, the public API (spec format, CLI, exit codes) or licensing. Ask the maintainer, and
carry on with work that doesn't depend on the answer.

## Local overrides

`local.just`, `lefthook-local.yml`, `CLAUDE.local.md`, `.claude/settings.local.json` and `.env.local` are personal and
gitignored. Never commit them, and
never put machine-specific paths, tools or secret-manager commands into committed files.

## Writing style

Precise, concise, plain English. No marketing language. Docs say what something does and when to use it.
