# 0010: No code signing

- Status: accepted
- Date: 2026-10-14

## Context and problem statement

toastback's exes are unsigned; code signing was out of scope for v1 ([0001](0001-baseline.md)). Windows 11's Smart
App Control may block unsigned programs, and it came up twice during development:

- In M3, Windows once showed "Smart App Control has blocked part of this app". Protection history was empty, and a
  timestamped rerun of the same steps (process enumeration, the delayed `cmd.exe` cleanup, a freshly copied exe)
  triggered nothing. Most likely Smart App Control briefly held a brand-new unsigned build while it looked up its
  reputation. Every dev build is new and unsigned, so the occasional alert is expected.
- In M4, on a machine with Smart App Control enforcing:

| Exe | Started from | Result |
|---|---|---|
| local dev build, copied by WSL `cp` or `curl` | `C:\Users\…`, `%TEMP%` | blocked ("An Application Control policy has blocked this file") |
| local dev build | the WSL file system, or the `bin\` folder `install` writes | runs |
| CI-built v0.1.0 release, downloaded by `gh` or `curl` | `%TEMP%` (`install.sh`) | **runs** |

Why the release build passes is unknown; it may have a reputation in Microsoft's cloud lookup that a fresh local build
doesn't.

## Considered options

1. Sign the release builds.
2. Don't sign, and document that Smart App Control *may* block them.

## Decision outcome

Chosen option: "don't sign", because setting up signing isn't straightforward, and unsigned release builds work in
practice (the v0.1.0 release ran under Smart App Control enforcing).

### Consequences

- The README says toastback isn't code-signed and that Smart App Control may block unsigned builds.
- Code signing is off the to-do list. If users report Smart App Control blocks, revisit this.
- Release integrity rests on `SHA256SUMS` and the build provenance attestation instead
  ([0012](0012-repository-and-release.md)).
