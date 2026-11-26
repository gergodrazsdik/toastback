# 0009: Installer and line endings: a script that survives a pipe, LF everywhere

- Status: accepted
- Date: 2026-10-14

## Context and problem statement

M4 set up CI and released v0.1.0 to test the whole process. Two problems showed up that local tests couldn't catch:

- **The `curl | bash` one-liner did nothing**: no output, exit 0. Piped into bash, the script *is* bash's stdin. The
  first `cmd.exe` call (finding `%TEMP%`) inherited that stdin through WSL interop and read the rest of the script, so
  bash hit end-of-input. Local tests ran `bash scripts/install.sh` from a file.
- **CRLF checkouts:** the Windows runner checks out with `core.autocrlf`, so `include_str!` embedded a CRLF shim
  template, and a CRLF shell script doesn't run in WSL. Builds on Linux, including every release, were never affected.

## Considered options

1. The installer: keep the top-level script and redirect stdin per call, or wrap everything in a function called on
   the last line.
2. Line endings: strip `\r` when rendering the shim, pin LF in `.gitattributes`, or both.

## Decision outcome

Chosen:

- **Everything in `main()`, called on the last line**, so the shell parses the whole script before running any of it.
  A truncated download is then a syntax error that runs nothing. Windows commands also get `</dev/null`.
- **Both:** `.gitattributes` pins LF (`* text=auto eol=lf`), and `render_shim` strips `\r` too.

### Consequences

- Test the installer piped, not only from a file: `cat scripts/install.sh | TOASTBACK_BASE_URL=file://<dir> sh`.
- The fixed `install.sh` shipped with 1.0.0. Since then the README doesn't pipe it into a shell at all: download it
  with `SHA256SUMS`, check it, read it, run it.

## More information

Also caught by CI: `doctor` reported `wsl.exe ok` with an empty distro list on the runner. It's a warning now.
