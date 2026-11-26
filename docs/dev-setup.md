# Development setup

What you need to work on toastback, and how to check it. toastback is a Windows program built in WSL: the code is
cross-compiled for `x86_64-pc-windows-msvc` with cargo-xwin, and the tests run the Windows build through WSL interop.

## Prerequisites

- **Windows 10 22H2 or later, or Windows 11, with WSL 2.** Building works on any Linux; the Windows tests, `just run`
  and `just install` need WSL interop.
- **Keep the clone in the Linux file system** (for example `~/projects/toastback`), not under `/mnt/c`: builds and git
  are much faster there, and line endings stay LF.

| Tool | Install |
|---|---|
| Git | <https://git-scm.com/downloads> |
| just | <https://just.systems/man/en/packages.html> |
| rustup | <https://rustup.rs>; `rust-toolchain.toml` picks the toolchain, the Windows target and the components |
| clang, lld, llvm | your distro's packages (`llvm-rc` and `lld-link` must be on PATH under these names) |
| cargo-xwin | `cargo install --locked cargo-xwin` |

The linters (`just lint` and the git hooks):

| Tool | Install |
|---|---|
| lefthook | <https://lefthook.dev/installation/> |
| ShellCheck | <https://github.com/koalaman/shellcheck#installing> |
| yamllint | `pipx install yamllint` or `uv tool install yamllint` |
| actionlint | <https://github.com/rhysd/actionlint/blob/main/docs/install.md> |
| zizmor | `pipx install zizmor` or `uv tool install zizmor`, <https://docs.zizmor.sh/installation/> |
| pinact | <https://github.com/suzuki-shunsuke/pinact#install> |
| markdownlint-cli2 | `npm install -g markdownlint-cli2` (needs Node.js) |
| typos | `cargo install typos-cli`, or <https://github.com/crate-ci/typos#install> |
| editorconfig-checker | <https://github.com/editorconfig-checker/editorconfig-checker#installation> |
| gitleaks | <https://github.com/gitleaks/gitleaks#installing> |

`just tools` lists what's installed and how to install the rest. `just lint` and the hooks skip a linter that's
missing, with a message; CI runs them all, at the versions pinned in `.github/workflows/lint.yml` and
`scripts/install-ci-tools.sh`.

Recommended: the [GitHub CLI](https://cli.github.com/) (`gh`) and [Claude Code](https://code.claude.com/docs/en/setup).
The winget recipes also need the Windows `winget` and `wingetcreate` (`just winget-setup` checks them), and the Scoop
recipes need [Scoop](https://scoop.sh) on Windows (`just scoop-setup` checks it).

## First steps

```sh
git clone https://github.com/gergodrazsdik/toastback.git
cd toastback
just tools    # what's installed, with install hints
just hooks    # install the git hooks
just setup    # the Windows target, and the MSVC CRT and Windows SDK for cargo-xwin (accepts Microsoft's licence)
just check    # rustfmt, clippy for the Windows target, and every linter
just test     # host tests, Windows tests through WSL interop, `validate` on every example
just install  # build and install for this distro and on the Windows PATH
toastback doctor
```

`just --list` shows every recipe; the README's [Development](../README.md#development) section explains them. Checks
that need a person at a Windows desktop are in [manual-test.md](manual-test.md).

## Optional: signed commits

Signing is welcome but not required. GitHub shows a signed commit as "Verified" once your public key is on your
account:

- [Generating a new GPG key](https://docs.github.com/en/authentication/managing-commit-signature-verification/generating-a-new-gpg-key)
  or [signing with an SSH key](https://docs.github.com/en/authentication/managing-commit-signature-verification/telling-git-about-your-signing-key#telling-git-about-your-ssh-key)
- Then `git config commit.gpgSign true` (and for SSH keys `git config gpg.format ssh` and
  `git config user.signingKey <path to your public key>`).

## Working with Claude Code

The committed `.claude/settings.json` turns on the Bash sandbox with a network allowlist and denies reading secrets.
Windows interop doesn't work inside the sandbox, so the recipes that start the exe have to run outside it;
[CLAUDE.md](../CLAUDE.md) lists them.
