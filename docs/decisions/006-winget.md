# 006: winget package

Date: 2026-10-31. Takes winget out of PLAN.md §2's "out of scope".

**Package:** `GergoDrazsdik.Toastback`, publisher `Gergő Drazsdik`.

## Zip with a nested exe, not a portable package

A winget portable package only copies files and adds an alias; there's no post-install step. So `toastback install`
wouldn't run: no registry keys, no shim, and exit code 4 on every notification. Once the user ran it by hand there'd be
two copies, with `winget upgrade` updating the one the `toastback://` handler doesn't use, and `winget uninstall`
leaving the registry keys and shims behind.

So the manifest points at the release zip, with `toastback-cli.exe` as the nested installer and
`install --quiet --add-to-path` as its silent switches. That keeps the "self-installing exe, no installer/MSI" decision:
winget runs the same `install` as every other method, which already handles upgrades and a running exe.

## What `install` changed for it

- **Apps & Features entry** (`HKCU\...\Uninstall\Toastback`), written by every install method. winget reads it for the
  version and for `QuietUninstallString`; the key name is the manifest's `ProductCode`. So `winget upgrade` also offers
  upgrades to `install.sh` and zip users, and those upgrades add toastback to PATH.
- **`--wsl default`**, the default for a first install from Windows: users mainly use one distro, and Docker Desktop
  adds a second one that shouldn't be touched. The default comes from the `*` in `wsl.exe -l -v`, since the header text
  is translated.
- **A re-install from Windows without `--wsl` reuses the install record.** Otherwise each `winget upgrade` would undo a
  `--wsl none` and leave shims in other distros outdated.
- **Docker Desktop's distros** (`docker-desktop*`) are left out of `all`, with a note. As the default distro, they
  mean no shim and a warning. Named explicitly, they're treated like any other.
- **Time limits on `wsl.exe`** (30 s to list, 120 s per command in a distro), since an unattended install must not hang.
- **`--quiet`** skips the test notification, which would otherwise appear on each upgrade.

Known limit: a shim removed by hand stays in the record, so the next upgrade writes it again.

## Publishing

Templates are in `packaging/winget/`; `scripts/winget.sh` and the `just winget-*` recipes render, test and submit them
with the Windows winget and wingetcreate. New versions go out through `just winget-update X.Y.Z` after the release, not
from CI, so the repository holds no PAT. CI can be added once the flow has proven itself.

Local tests need `winget settings --enable LocalManifestFiles` once, in an elevated prompt. `just winget-test` serves
the local zip over `http://localhost`, because a manifest needs a URL, and replaces the install on the machine, so it
ends with `just install`.
