# 0021: An agent skill in the exe: `toastback skill` and `install --skill`

- Status: accepted
- Date: 2026-11-28

## Context and problem statement

Coding agents (Claude Code, Codex and others) that use toastback learn it from `--help`, `toastback examples` and
the README, or from a skill someone wrote for their own machine. They miss what matters most to a caller: which form
to choose (fire-and-forget, `--wait`, a progress bar with `update`, `notify-send`), the exit codes, and the usual
mistakes (a button without `run`, id rules, running inside a sandbox that blocks Windows interop, services without
your PATH). A skill kept outside the repository falls behind with each release. The goal: a skill that always
matches the installed version.

## Considered options

1. Where the skill lives: on each machine, written by hand; in the repository, as `skills/toastback/SKILL.md`,
   embedded in the exe; or in a Claude Code plugin marketplace that installs it from the repository.
2. How it reaches the agent: printed by a command, written by `install`, or both.
3. `install` writes it by default, or only when asked.
4. Where `install` writes it: the WSL distros that get the shim, Windows' `%USERPROFILE%\.claude\skills`, or both.
5. How the skill is kept current: by review, or by tests against the code.

## Decision outcome

Chosen:

- **In the repository, embedded in the exe** (option 1.2). `skills/toastback/SKILL.md` is in the shared `SKILL.md`
  format (frontmatter with `name` and `description`, then Markdown) and written for any agent, not only Claude Code.
  The exe embeds it (`src/skill.rs`) with the version filled in, as it embeds the examples. A marketplace plugin
  installs from the default branch, which documents what's under `[Unreleased]`, so it doesn't match the installed
  version; pinning it to the latest tag would change the release flow ([0012](0012-repository-and-release.md)). It
  can come later, on top of the same file.
- **Both** (option 2). `toastback skill` prints it, for any agent or folder
  (`toastback skill > <dir>/toastback/SKILL.md`). `install --skill` writes it to `~/.claude/skills/toastback/SKILL.md`,
  where Claude Code looks for personal skills. A file there also lets an agent read the skill without starting the
  Windows exe, which a sandboxed agent can't do.
- **Only when asked** (option 3). The notify-send wrapper is written by default because it replaces a command that
  doesn't work in WSL; this writes into another program's settings folder. `install --skill` is recorded (`skill` in
  `install.json`), so later installs and upgrades (`install.sh`, winget, Scoop) rewrite it for the new version.
  `install --no-skill` removes ours and turns it off.
- **The WSL distros that get the shim** (option 4.1), for the user the shim is for, in a separate `wsl.exe` call
  with the skill on stdin: the skill is too long to embed in the `sh -c` script safely, given Windows' command-line
  limit. Windows-side agents can use `toastback-cli skill` for now.
- **Marked and recorded like the wrapper** ([0006](0006-install-and-uninstall.md), [0017](0017-notify-send.md)).
  The frontmatter has to come first, so the marker `<!-- toastback skill` is a comment within the first 12 lines.
  `install` writes the file only when nothing is there or the file there carries the marker; anything else is left
  alone with a note. `uninstall`, `install --wsl none` and `install --no-skill` remove ours, and its directory if
  that's empty then. A failed skill write is a warning: the shim is still recorded. With `--skill` on, `doctor`
  reports whether the caller's distro has the current skill.
- **Tests** (option 5) fail when the skill and the code part: every command and long flag the skill names in code
  must exist, every visible command and long flag must be named in it, every ```` ```json ```` block must be a valid
  spec, every field of the spec, of a callback, a button, a progress bar and the context must be named, and the exit
  code table must list exactly the exit codes the code defines.

### Consequences

- A new command, flag, spec field or exit code fails `cargo test` until the skill covers it. Renaming or removing
  one fails it too.
- `toastback skill` is a command now, so it no longer means "show the spec file named `skill`", as for every other
  command name. Additive otherwise: a minor release.
- The skill isn't written for Windows-side agents, other agents' folders or other users in a distro. A later record
  can add destinations.

## More information

Claude Code reads personal skills from `~/.claude/skills/<name>/SKILL.md`. The file's text is the skill, so it's
written in the repository's plain style and kept short enough to load whole.
