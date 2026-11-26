# Architecture decision records

Decisions about toastback's design, behaviour and process. The [plan](../plan.md) is the starting point; ADRs record
what was decided after it, refine it and supersede it. A decision is recorded as an ADR in the pull request that
makes it.

The first fourteen were rewritten on 2026-11-26 from the milestone notes kept in `docs/decisions/` until then
(`000-spikes.md` to `007-posix-sh.md`), split by topic. Their dates are those of the original notes.

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-baseline.md) | Baseline: the plan's locked decisions | accepted, amended by 0003, 0013 |
| [0002](0002-platform-spikes.md) | Platform spikes: registry-only app id, protocol activation, TaskDialog, cross-built GUI exe | accepted |
| [0003](0003-terminal-output.md) | Terminal output: attach the parent console, plus a console-subsystem `toastback-cli.exe` | accepted |
| [0004](0004-running-callbacks.md) | Running callbacks: `wsl.exe --exec`, encoded PowerShell, raw cmd, one click per notification | accepted, amended by 0014 |
| [0005](0005-spec-and-cli.md) | Spec and CLI: the spec's context wins, `show` is the default command, WSL paths | accepted |
| [0006](0006-install-and-uninstall.md) | Install and uninstall: recorded shims, marked files, PATH, uninstall while running | accepted |
| [0007](0007-dialogs.md) | Dialogs: TaskDialog mapping, on top, a detached dialog process, one callback path | accepted |
| [0008](0008-commands-and-click-feedback.md) | Commands and click feedback: `validate`, `remove`, `clear`, `doctor`, `notify_on_success` | accepted |
| [0009](0009-installer-and-line-endings.md) | Installer and line endings: a script that survives a pipe, LF everywhere | accepted |
| [0010](0010-no-code-signing.md) | No code signing | accepted |
| [0011](0011-shim-without-distro-name.md) | Shim v2: the distro from `wslpath` when `WSL_DISTRO_NAME` is missing | accepted, amended by 0014 |
| [0012](0012-repository-and-release.md) | Repository settings, release flow, changelog and versions | accepted, amended by 0016 |
| [0013](0013-winget.md) | winget: the release zip with a nested installer | accepted |
| [0014](0014-posix-sh.md) | No bash needed in WSL | accepted |
| [0015](0015-pinned-ci.md) | Pinned CI: actions by commit SHA, linters by version and digest, a 7-day cooldown | accepted |
| [0016](0016-settings-as-code.md) | Repository settings as code | accepted |

## Numbering and status

- Files are named `NNNN-short-title.md`: four digits, in order, never reused.
- Status is one of `proposed`, `accepted`, `amended by NNNN`, `superseded by NNNN` or `deprecated`.
- An accepted ADR isn't edited to change its decision: a new ADR amends or supersedes it, and the old one's status
  points to the new one. Fixing typos and links is fine.
- Group related small decisions into one ADR rather than writing one file per tiny choice.
- Add the ADR to the index above in the same pull request.

## Template

[MADR](https://adr.github.io/madr/)-style:

```markdown
# NNNN: Title

- Status: proposed | accepted | amended by NNNN | superseded by NNNN | deprecated
- Date: YYYY-MM-DD

## Context and problem statement

## Considered options

## Decision outcome

Chosen option: "…", because …

### Consequences

## More information
```
