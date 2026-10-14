# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub's
[private vulnerability reporting](https://github.com/gergodrazsdik/toastback/security/advisories/new), not in a
public issue. Include what you found, how to reproduce it, and what it lets an attacker do.

This is a hobby project maintained by one person, so there's no guaranteed response time, but reports are taken
seriously and fixed releases are published as soon as practical.

## Supported versions

Only the latest release gets fixes.

## Scope

Of particular interest: anything that makes a click run a command the notification's author didn't specify
(activation URIs, the pending store, tokens), command or argument injection in callbacks, and the installer
(`install.sh`, `toastback.exe install`).
