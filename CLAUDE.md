@AGENTS.md

## Claude Code

Additions for Claude Code only; everything shared is in AGENTS.md (imported above, so it loads once whether or not
Claude Code also loads `AGENTS.md` on its own).

- **Sandbox:** `.claude/settings.json` turns on the Bash sandbox with a network allowlist (GitHub, crates.io, the
  Rust toolchain downloads) and denies reading or editing `.env*` files, keys, certificates and `secrets/`. Signing,
  SSH, the GitHub token and Windows interop aren't available inside it, so these run outside, as their own calls,
  as your user-level settings allow:
  - commits: `git commit -F <file>`, with the message in a file outside the repo;
  - pushing and fetching: `git push`, `git pull`;
  - GitHub: `gh` (pull requests with `gh pr create --body-file <file>`, `gh pr edit --body-file <file>`), `gh stack`;
  - anything that starts a Windows exe: `just test`, `just run`, `just install`, `just uninstall`, `just demo`,
    `just log`, `just doctor`, the `just winget-*` recipes, `toastback`, and the built exes under `target/`.

  A call only runs outside when the whole command matches: no `cd … &&`, pipes, heredocs, `$(...)` or redirections
  around it. Project settings can't add sandbox exclusions; they live in each user's own settings.
- **Subagents:** give each one its own git worktree and branch, explicit file ownership and its own pull request.
  Shared files (AGENTS.md, CHANGELOG.md) belong to the main agent; subagents propose changes to them in their pull
  request description.
- **Check what loads:** `claude -p --permission-mode plan "list the instruction files in your context"` from the repo
  root.
