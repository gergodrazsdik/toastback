# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub's
[private vulnerability reporting](https://github.com/gergodrazsdik/toastback/security/advisories/new): on the
repository's **Security** tab, choose **Report a vulnerability**. Do not open a public issue, pull request or
discussion about it.

Include what you found, how to reproduce it, and what it lets an attacker do.

## Scope

In scope, and of particular interest:

- Anything that makes a click run a command the notification's author didn't specify: activation URIs, the pending
  store, tokens.
- Command or argument injection in callbacks.
- The installer (`install.sh`, `toastback.exe install`), the WSL shim, and the registry entries and files they write.
- The repository's workflows, scripts and settings, for example a workflow that leaks a token or that an outside
  contributor can trigger, or a release asset that doesn't match its `SHA256SUMS` or attestation.

Out of scope:

- The commands a spec asks toastback to run. Showing a spec from an untrusted source is like running its script:
  toastback runs what the spec says, by design.
- Problems in a user's own configuration, distro or Windows setup that toastback's defaults don't cause.
- Vulnerabilities in dependencies that are already public and tracked upstream, unless toastback uses them in an
  exploitable way.

## Supported versions

Only the latest release gets fixes.

## Response

This is a hobby project maintained by one person, so there's no guaranteed response time, but reports are taken
seriously. Fixed releases are published as soon as practical, and fixed vulnerabilities as GitHub security advisories,
with credit if you want it.
