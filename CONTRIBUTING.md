# Contributing to toastback

Thanks for your interest. toastback has one maintainer, so the process is light. Two rules are strict: every change
goes through a pull request with green CI, and nothing personal or machine-specific goes into the repo.

What toastback does and how to use it is in the [README](README.md). The original design is in
[docs/plan.md](docs/plan.md), the decisions made since in [docs/adr/](docs/adr/README.md), and what changed in
[CHANGELOG.md](CHANGELOG.md).

## Proposing changes

- **Small fixes** (typos, broken links, an obvious bug): open a pull request directly.
- **Anything non-trivial** (a feature, a new spec field or command, a dependency, a change to the process): open an
  issue first and agree on the approach, so nobody's work is wasted. Check the README's
  [Limitations](README.md#limitations), the ADRs and the plan's §12 ("Later") first: it may be a Windows limitation,
  or already decided.
- **Security problems:** never in a public issue. See [SECURITY.md](SECURITY.md).

Since 1.0.0 the spec format, the CLI and the exit codes are a public API: a change that breaks them needs a very good
reason and a major version.

## Branches and pull requests

- Work on a branch in your fork (`feat/<topic>`, `fix/<topic>`, `docs/<topic>`…). `main` only changes through pull
  requests whose CI passes.
- Open a pull request using the template. The maintainer merges it by **squash**: its title and description become the
  commit on `main`, so give it a short imperative title ("Fix the shim for distros without bash") and a description
  that says what changed and why.
- Keep the branch up to date with `main` (the "Update branch" button, or `git rebase main` and
  `git push --force-with-lease` to your branch).
- A change people would notice adds a line under **Unreleased** in [CHANGELOG.md](CHANGELOG.md), in the same pull
  request ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/): Added, Changed, Deprecated, Removed, Fixed,
  Security).
- A new spec field or command also updates the README, adds or extends an example in `examples/` (registered in
  `src/examples.rs`) and adds a step to [docs/manual-test.md](docs/manual-test.md).
- **Stacked changes:** the maintainer and agents use the
  [`gh stack`](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs) extension for pull requests
  that build on each other. Without it, open ordinary pull requests and say in the description which one yours builds
  on.

## Licensing of contributions

toastback is licensed [GPL-3.0-or-later](LICENSE). There is no Contributor License Agreement: you keep the copyright in
your contribution, and by submitting it you license it under GPL-3.0-or-later, the same terms everyone gets the
project under.

- Submit only work you have the right to license that way. Work you create as part of your job usually belongs to your
  employer: contribute it only with their approval.
- **Third-party material** (code, text or images by someone else, or generated output whose terms you are bound by):
  say so in the pull request description (what it is, where it comes from, who holds the copyright and under which
  licence you received it), keep its copyright and licence notices intact, and submit it only if its licence is
  compatible with GPL-3.0-or-later. The maintainer decides whether it can be included.

## Signing

Signed commits are welcome but not required: the squash commit on `main` is made and signed by GitHub. To sign, see
[About commit signature verification](https://docs.github.com/en/authentication/managing-commit-signature-verification/about-commit-signature-verification).

## Local setup

See [docs/dev-setup.md](docs/dev-setup.md) for the prerequisites. Then:

```sh
just tools    # check the tools, with install hints
just hooks    # install the git hooks
just setup    # once: the Windows target and the MSVC CRT and SDK
just check    # rustfmt, clippy and the linters CI runs (`just lint`)
just test     # the tests on Linux and, through WSL interop, on Windows
just install  # build and install for this distro
```

`just lint` and the hooks skip a linter that isn't installed and print a clear message; CI sets
`TOASTBACK_REQUIRE_TOOLS=1` and runs every check.

### Local overrides

These files are personal and gitignored. Put anything machine-specific or secret-related there, never in committed
files:

| File | Purpose |
|---|---|
| `local.just` | Extra recipes, imported by the justfile if present (see `local.just.example`). |
| `lefthook-local.yml` | Personal hook overrides ([lefthook docs](https://lefthook.dev/configuration/)). |
| `CLAUDE.local.md` | Personal notes for Claude Code. |
| `.claude/settings.local.json` | Personal Claude Code settings. |
| `.env.local` | Local environment variables. |

## What goes in the repo

Committed files never contain personal or machine-specific data: no absolute paths, hostnames, usernames, distro
names, secret-manager entries or credentials.

Every source file starts with the licence identifier as a comment, on the first line or right after a shebang:

```text
// SPDX-License-Identifier: GPL-3.0-or-later
```

(`#` instead of `//` in shell scripts.)

## Workflow approvals

Workflows from first-time and outside contributors need the maintainer's approval before they run. That is a GitHub
setting for every outside contributor, not a judgement on you.

## AI-assisted contributions

Contributions written with AI coding agents are welcome; much of this project is built that way. You are responsible
for every line you submit: review it, understand it, test it. Agent instructions for this repo are in
[AGENTS.md](AGENTS.md).

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating you agree to uphold it.
