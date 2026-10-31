// SPDX-License-Identifier: GPL-3.0-or-later
//! `install`, `uninstall`, the WSL shim and the user PATH entry.

use anyhow::{Context as _, Result, ensure};

use crate::context::same_distro;
use crate::spec::Context;
use crate::store::{InstallRecord, ShimTarget};

const SHIM_TEMPLATE: &str = include_str!("../res/shim.sh.tmpl");
/// First-line marker: `uninstall` only deletes files that carry it.
pub const SHIM_MARKER: &str = "# toastback WSL shim";
pub const GUI_EXE: &str = "toastback.exe";
pub const CLI_EXE: &str = "toastback-cli.exe";
/// Must match `Publisher` in the winget manifest and `CompanyName` in `res/toastback.rc`.
pub const PUBLISHER: &str = "Gergő Drazsdik";

/// Render the WSL shim for an installed exe at `win_exe` (a Windows path).
pub fn render_shim(version: &str, win_exe: &str) -> String {
    // the path sits in single quotes: close, escaped quote, reopen
    let quoted = win_exe.replace('\'', r"'\''");
    // LF only, even if the template was checked out with CRLF: it runs in sh
    SHIM_TEMPLATE
        .replace('\r', "")
        .replace("{{VERSION}}", version)
        .replace("{{WIN_EXE_PATH}}", &quoted)
}

/// The protocol handler command registered for `toastback://`.
pub fn protocol_command(exe: &str) -> String {
    format!("\"{exe}\" activate \"%1\"")
}

/// The silent uninstall command registered in Apps & Features.
pub fn uninstall_command(exe: &str) -> String {
    format!("\"{exe}\" uninstall")
}

/// The default distro in `wsl.exe -l -v` output: the line marked `*` (the header is translated, the
/// marker isn't). Distro names contain no spaces, so the name is the first word after it.
pub fn parse_default_distro(bytes: &[u8]) -> Option<String> {
    crate::exec::decode_output(bytes).lines().find_map(|l| {
        l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0')
            .strip_prefix('*')?
            .split_whitespace()
            .next()
            .map(str::to_string)
    })
}

/// Docker Desktop's own distros: `--wsl all` and `--wsl default` leave them alone.
pub fn is_docker_distro(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("docker-desktop")
}

/// Parse `wsl.exe -l -q` output: UTF-16LE (with BOM/NULs) by default, UTF-8 with `WSL_UTF8=1`.
pub fn parse_distro_list(bytes: &[u8]) -> Vec<String> {
    crate::exec::decode_output(bytes)
        .lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0'))
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Is `name` in a list of distros (names are case-insensitive)?
pub fn has_distro(list: &[String], name: &str) -> bool {
    list.iter().any(|d| same_distro(d, name))
}

/// Where `install` writes the shim, and the distros `--wsl all` left out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ShimPlan {
    pub targets: Vec<ShimTarget>,
    pub skipped: Vec<String>,
}

/// Where `install --wsl <wsl>` writes the shim. The calling distro always gets the calling user
/// (with `all` too); other distros get their default user. Without `--wsl`: the calling distro,
/// else what the install record lists (an upgrade keeps the user's choice, `--wsl none` too), else
/// the default distro. A named distro must exist. `recorded` is `None` on a first install. `list`
/// and `default` run only when needed.
pub fn shim_targets(
    wsl: Option<&str>,
    ctx: &Context,
    recorded: Option<&InstallRecord>,
    list: impl FnOnce() -> Result<Vec<String>>,
    default: impl FnOnce() -> Result<Option<String>>,
) -> Result<ShimPlan> {
    let target = |distro: String| ShimTarget {
        user: ctx.user.clone().filter(|_| {
            ctx.distro
                .as_deref()
                .is_some_and(|c| same_distro(c, &distro))
        }),
        distro,
    };
    let default_target = || -> Result<ShimTarget> {
        let d = default()?.context("WSL has no default distro")?;
        ensure!(
            !is_docker_distro(&d),
            "the default WSL distro is Docker Desktop's ({d}): choose one with --wsl NAME"
        );
        Ok(target(d))
    };
    let one = |t| ShimPlan {
        targets: vec![t],
        skipped: Vec::new(),
    };
    Ok(match (wsl, &ctx.distro, recorded) {
        (Some("none"), ..) => ShimPlan::default(),
        (Some("all"), ..) => {
            let (skipped, distros) = list()?.into_iter().partition(|d| is_docker_distro(d));
            let distros: Vec<String> = distros;
            ShimPlan {
                targets: distros.into_iter().map(target).collect(),
                skipped,
            }
        }
        (Some("default"), ..) => one(default_target()?),
        (Some(d), ..) => {
            let existing = list()?;
            ensure!(
                has_distro(&existing, d),
                "no WSL distro named {d} (there are: {})",
                existing.join(", ")
            );
            one(target(d.into()))
        }
        (None, Some(d), _) => one(target(d.clone())),
        (None, None, Some(r)) if r.shims_declined() => ShimPlan::default(),
        (None, None, Some(r)) => {
            let existing = list()?;
            let targets: Vec<ShimTarget> = r
                .shims
                .iter()
                .filter(|t| has_distro(&existing, &t.distro))
                .cloned()
                .collect();
            if targets.is_empty() {
                // nothing recorded worked out (a first install without WSL, removed distros)
                one(default_target()?)
            } else {
                ShimPlan {
                    targets,
                    skipped: Vec::new(),
                }
            }
        }
        (None, None, None) => one(default_target()?),
    })
}

fn same_dir(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\')
        .eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

/// A running exe moved aside: it can't be replaced or deleted, only renamed. The name still ends in
/// `.exe`: `uninstall` may print it as a command, and PowerShell only runs a file with that ending
/// as a program (anything else opens the "Open with" window).
pub fn moved_aside_name(exe_stem: &str, pid: u32) -> String {
    format!("{exe_stem}.old-{pid}.exe")
}

/// Is this file in the bin dir left over from replacing an exe? (`.exe.old-<pid>`: before 1.4.)
pub fn is_stale_copy(name: &str) -> bool {
    name.contains(".old-") || name.contains(".exe.new-")
}

/// Is `dir` one of the `;`-separated entries of a PATH value?
pub fn path_contains(path: &str, dir: &str) -> bool {
    path.split(';').any(|p| same_dir(p, dir))
}

/// PATH with `dir` appended, or `None` if it's already there.
pub fn path_with(path: &str, dir: &str) -> Option<String> {
    if path_contains(path, dir) {
        return None;
    }
    let base = path.trim_end_matches(';');
    Some(if base.is_empty() {
        dir.to_string()
    } else {
        format!("{base};{dir}")
    })
}

/// PATH without `dir`, or `None` if it wasn't there.
pub fn path_without(path: &str, dir: &str) -> Option<String> {
    if !path_contains(path, dir) {
        return None;
    }
    Some(
        path.split(';')
            .filter(|p| !p.is_empty() && !same_dir(p, dir))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use std::fs;
    use std::io::Write as _;
    use std::os::windows::process::CommandExt as _;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Output, Stdio};
    use std::time::{Duration, Instant};

    use anyhow::{Context as _, Result, anyhow, bail, ensure};
    use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    use windows::core::{PWSTR, w};

    use super::{
        CLI_EXE, GUI_EXE, PUBLISHER, SHIM_MARKER, has_distro, is_stale_copy, moved_aside_name,
        parse_default_distro, parse_distro_list, path_with, path_without, protocol_command,
        render_shim, same_distro, uninstall_command,
    };
    use crate::exec::{Drain, OUTPUT_GRACE};
    use crate::registry::{self, Kind};
    use crate::spec::Context;
    use crate::store::{InstallRecord, ShimTarget, Store, timestamp};

    use crate::toast::{self, AUMID};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    pub const PROTOCOL_KEY: &str = r"Software\Classes\toastback";
    pub const PROTOCOL_COMMAND_KEY: &str = r"Software\Classes\toastback\shell\open\command";
    pub const ENV_KEY: &str = "Environment";
    /// The Apps & Features entry. Its name is the winget manifest's `ProductCode`.
    pub const APP_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Toastback";
    /// `wsl.exe -l`: doesn't start a distro, so it's quick unless WSL itself is stuck.
    const LIST_TIMEOUT: Duration = Duration::from_secs(30);
    /// One command in a distro, which may have to boot first. `install` runs unattended under
    /// winget, so a stuck distro must not hang it.
    const SHIM_TIMEOUT: Duration = Duration::from_secs(120);

    pub fn aumid_key() -> String {
        format!(r"Software\Classes\AppUserModelId\{AUMID}")
    }

    pub struct Options<'a> {
        /// A distro name, `all`, `default`, `none`, or `None`: see [`super::shim_targets`].
        pub wsl: Option<&'a str>,
        pub add_to_path: bool,
    }

    pub fn install(store: &Store, opts: &Options, ctx: &Context, version: &str) -> Result<()> {
        // one listing (wsl.exe can take a while) for the shim targets and the install record
        let distros = list_distros();
        let listed = || match &distros {
            Ok(d) => Ok(d.clone()),
            Err(e) => Err(anyhow!("{e:#}")),
        };
        let previous = readable_record(store);
        let previously_declined = previous.as_ref().is_some_and(InstallRecord::shims_declined);
        // Where the shim goes, before anything changes: an explicit `--wsl all` that can't list
        // the distros fails here, with nothing half-installed. By default, no WSL is a warning.
        let plan = super::shim_targets(opts.wsl, ctx, previous.as_ref(), listed, default_distro);
        let plan = match plan {
            Ok(p) => p,
            Err(e) if opts.wsl.is_none() => {
                errln!("warning: no WSL shim written: {e:#}");
                super::ShimPlan::default()
            }
            Err(e) => return Err(e),
        };
        for d in &plan.skipped {
            outln!("skipped {d}: it's Docker Desktop's (name it with --wsl to include it)");
        }
        let targets = plan.targets;
        // both exes ship side by side; install from whichever one is running
        let me = std::env::current_exe()?;
        let src_dir = me.parent().context("exe has no directory")?;
        let gui = src_dir.join(GUI_EXE);
        ensure!(
            gui.exists(),
            "{GUI_EXE} must be next to {} to install",
            me.display()
        );
        fs::create_dir_all(store.bin_dir())?;
        remove_stale_copies(&store.bin_dir());
        let exe = store.installed_exe();
        copy_exe(&gui, &exe)?;
        let cli = src_dir.join(CLI_EXE);
        let installed_cli = store.bin_dir().join(CLI_EXE);
        if cli.exists() {
            copy_exe(&cli, &installed_cli)?;
        } else if installed_cli.exists() {
            errln!(
                "warning: {CLI_EXE} isn't next to {}, so the one in {} keeps its old version",
                me.display(),
                store.bin_dir().display()
            );
        }
        outln!(
            "installed {} (written fresh, so no \"downloaded from the internet\" marker carries over)",
            store.bin_dir().display()
        );
        let icon = store.bin_dir().join("icon.png");
        fs::write(&icon, include_bytes!("../res/icon.png"))?;

        let aumid = aumid_key();
        registry::set_string(&aumid, "DisplayName", "toastback", Kind::Plain)?;
        registry::set_string(&aumid, "IconUri", &icon.to_string_lossy(), Kind::Plain)?;
        registry::set_string(PROTOCOL_KEY, "", "URL:toastback", Kind::Plain)?;
        registry::set_string(PROTOCOL_KEY, "URL Protocol", "", Kind::Plain)?;
        registry::set_string(
            PROTOCOL_COMMAND_KEY,
            "",
            &protocol_command(&exe.to_string_lossy()),
            Kind::Plain,
        )?;
        register_app(store, version)?;
        outln!(
            "registered the toastback:// handler, the notification app id and the Apps & Features entry"
        );

        let mut record = previous.unwrap_or_default();
        // forget shims in distros that were removed since (`wsl --unregister`)
        if let Ok(existing) = &distros {
            record.shims.retain(|t| has_distro(existing, &t.distro));
        }
        if opts.add_to_path {
            if add_to_path(&store.bin_dir().to_string_lossy())? {
                outln!(
                    "added {} to your user PATH (new terminals see it)",
                    store.bin_dir().display()
                );
                // only an entry we added is ours to remove on uninstall
                record.path_added = true;
            } else {
                outln!("{} is already on your user PATH", store.bin_dir().display());
            }
        }

        record.no_shims = Some(match opts.wsl {
            Some("none") => {
                // a shim that can't be removed now stays listed, so uninstall still tries it
                record.shims.retain(|t| !remove_shim(t));
                true
            }
            _ if !targets.is_empty() => false,
            _ => previously_declined,
        });

        let shim = render_shim(version, &exe.to_string_lossy());
        let mut failed = Vec::new();
        for t in &targets {
            match write_shim(t, &shim) {
                Ok(()) => {
                    // the same distro under another spelling replaces the old entry
                    record
                        .shims
                        .retain(|r| !(same_distro(&r.distro, &t.distro) && r.user == t.user));
                    record.shims.push(t.clone());
                }
                Err(e) => {
                    errln!("warning: shim for {}: {e:#}", t.distro);
                    failed.push(t.distro.as_str());
                }
            }
        }
        record.version = version.into();
        record.installed_at = timestamp(std::time::SystemTime::now());
        record.exe = exe.to_string_lossy().into_owned();
        store.save_install_record(&record)?;
        // asked for by name, it's a failure; on its own (an upgrade) a warning, like no WSL at all
        ensure!(
            opts.wsl.is_none() || failed.is_empty(),
            "no WSL shim written to {}",
            failed.join(", ")
        );
        Ok(())
    }

    /// The install record, or `None` with a warning if it can't be read: install and uninstall are
    /// the way out of a damaged one, so it must not stop them.
    fn readable_record(store: &Store) -> Option<InstallRecord> {
        store.install_record().unwrap_or_else(|e| {
            errln!("warning: ignoring the install record: {e:#}");
            None
        })
    }

    /// The entry winget and Apps & Features use to see the version and to uninstall.
    fn register_app(store: &Store, version: &str) -> Result<()> {
        let exe = store.installed_exe();
        let exe_str = exe.to_string_lossy();
        let uninstall = uninstall_command(&exe_str);
        let values = [
            ("DisplayName", "toastback"),
            ("DisplayVersion", version),
            ("Publisher", PUBLISHER),
            ("DisplayIcon", &exe_str),
            ("UninstallString", &uninstall),
            ("QuietUninstallString", &uninstall),
            ("URLInfoAbout", env!("CARGO_PKG_REPOSITORY")),
        ];
        let bin = store.bin_dir();
        let bin_str = bin.to_string_lossy();
        registry::set_string(APP_KEY, "InstallLocation", &bin_str, Kind::Plain)?;
        for (name, value) in values {
            registry::set_string(APP_KEY, name, value, Kind::Plain)?;
        }
        registry::set_dword(APP_KEY, "NoModify", 1)?;
        registry::set_dword(APP_KEY, "NoRepair", 1)?;
        let bytes: u64 = fs::read_dir(&bin)?
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        let kb = u32::try_from(bytes / 1024).unwrap_or(u32::MAX);
        registry::set_dword(APP_KEY, "EstimatedSize", kb)
    }

    /// Files left over from replacing an exe.
    fn remove_stale_copies(bin: &Path) {
        for entry in fs::read_dir(bin).into_iter().flatten().flatten() {
            if is_stale_copy(&entry.file_name().to_string_lossy()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// Copy an exe as fresh bytes, so the download marker (Zone.Identifier) doesn't come along. The
    /// old exe stays in place until the new one is complete: clicks and the shim depend on it.
    pub(super) fn copy_exe(src: &Path, dest: &Path) -> Result<()> {
        if dest.exists() && fs::canonicalize(src)? == fs::canonicalize(dest)? {
            return Ok(()); // installing from the installed copy
        }
        let bytes = fs::read(src).with_context(|| format!("read {}", src.display()))?;
        let pid = std::process::id();
        let new = dest.with_extension(format!("exe.new-{pid}"));
        let replaced = fs::write(&new, bytes)
            .with_context(|| format!("write {}", new.display()))
            .and_then(|()| replace_exe(&new, dest, pid));
        if replaced.is_err() {
            let _ = fs::remove_file(&new);
        }
        replaced
    }

    /// Put the finished `new` in `dest`'s place.
    fn replace_exe(new: &Path, dest: &Path, pid: u32) -> Result<()> {
        if fs::rename(new, dest).is_ok() {
            return Ok(());
        }
        // in use (a callback is running): a running exe can be renamed, not replaced
        let stem = dest.file_stem().unwrap_or_default().to_string_lossy();
        let old = dest.with_file_name(moved_aside_name(&stem, pid));
        fs::rename(dest, &old)
            .with_context(|| format!("move the running {} aside", dest.display()))?;
        fs::rename(new, dest).map_err(|e| {
            let _ = fs::rename(&old, dest);
            anyhow!(e).context(format!("replace {}", dest.display()))
        })
    }

    /// Append `dir` to the user PATH. Returns false if it was there already.
    fn add_to_path(dir: &str) -> Result<bool> {
        let (path, kind) =
            registry::get_string(ENV_KEY, "Path")?.unwrap_or((String::new(), Kind::Expand));
        let Some(new) = path_with(&path, dir) else {
            return Ok(false);
        };
        registry::set_string(ENV_KEY, "Path", &new, kind)?;
        broadcast_environment_change();
        Ok(true)
    }

    fn remove_from_path(dir: &str) -> Result<bool> {
        let Some((path, kind)) = registry::get_string(ENV_KEY, "Path")? else {
            return Ok(false);
        };
        let Some(new) = path_without(&path, dir) else {
            return Ok(false);
        };
        registry::set_string(ENV_KEY, "Path", &new, kind)?;
        broadcast_environment_change();
        Ok(true)
    }

    /// Tell Explorer (and new terminals it starts) that the user environment changed.
    fn broadcast_environment_change() {
        // SAFETY: the lParam string is a static NUL-terminated wide string.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(w!("Environment").as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                5000,
                None,
            );
        }
    }

    /// A program in System32, by full path. A bare name would look in the running exe's folder
    /// first: a `wsl.exe` planted in Downloads would run when `install` is started from there.
    pub(super) fn system_exe(name: &str) -> Result<PathBuf> {
        let mut dir = [0u16; 260];
        // SAFETY: the slice is a local buffer, and the call writes within its length.
        let len = unsafe { GetSystemDirectoryW(Some(&mut dir)) } as usize;
        ensure!(
            len > 0 && len < dir.len(),
            "can't find the system directory"
        );
        Ok(PathBuf::from(String::from_utf16_lossy(&dir[..len])).join(name))
    }

    /// Wait for a child with piped stdout and stderr, and kill it after `limit`.
    pub(super) fn wait_limited(mut child: Child, limit: Duration, what: &str) -> Result<Output> {
        let piped = "the child's output isn't piped";
        let stdout = Drain::start(child.stdout.take().context(piped)?, usize::MAX);
        let stderr = Drain::start(child.stderr.take().context(piped)?, usize::MAX);
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() >= limit {
                let _ = child.kill();
                let _ = child.wait();
                // not waiting for the readers: whatever the child started may still hold the pipes
                bail!("{what}: no answer after {} s", limit.as_secs());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        // the same goes after a normal exit
        let deadline = Instant::now() + OUTPUT_GRACE;
        Ok(Output {
            status,
            stdout: stdout.collect_bytes(deadline),
            stderr: stderr.collect_bytes(deadline),
        })
    }

    /// Stdout of `wsl.exe <args>`.
    fn wsl_list(args: &[&str]) -> Result<Vec<u8>> {
        let what = format!("wsl.exe {}", args.join(" "));
        let child = Command::new(system_exe("wsl.exe")?)
            .args(args)
            .env("WSL_UTF8", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .with_context(|| format!("run {what}"))?;
        let out = wait_limited(child, LIST_TIMEOUT, &what)?;
        ensure!(out.status.success(), "{what} failed");
        Ok(out.stdout)
    }

    pub fn list_distros() -> Result<Vec<String>> {
        Ok(parse_distro_list(&wsl_list(&["-l", "-q"])?))
    }

    /// The distro `wsl.exe` starts by default, if there is one.
    pub fn default_distro() -> Result<Option<String>> {
        Ok(parse_default_distro(&wsl_list(&["-l", "-v"])?))
    }

    /// Run `sh -c script` in a distro, optionally feeding `stdin`. Returns stdout.
    pub fn wsl_sh(t: &ShimTarget, script: &str, stdin: Option<&str>) -> Result<String> {
        let mut c = Command::new(system_exe("wsl.exe")?);
        c.args(["-d", &t.distro]);
        if let Some(u) = &t.user {
            c.args(["-u", u]);
        }
        let mut child = c
            .args(["--exec", "sh", "-c", script])
            .env("WSL_UTF8", "1")
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("start wsl.exe")?;
        // A failed write usually means wsl.exe has exited: its own error says more, so wait for it.
        let written = match (stdin, child.stdin.take()) {
            (Some(input), Some(mut pipe)) => pipe.write_all(input.as_bytes()),
            _ => Ok(()),
        };
        let what = format!("wsl.exe -d {}", t.distro);
        let out = wait_limited(child, SHIM_TIMEOUT, &what)?;
        if !out.status.success() {
            bail!(
                "exit {:?}: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        written.context("send the script's input to wsl.exe")?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The shim currently installed in a distro, if any.
    pub fn read_shim(t: &ShimTarget) -> Result<Option<String>> {
        let out = wsl_sh(
            t,
            r#"f="$HOME/.local/bin/toastback"; if [ -f "$f" ]; then printf y; cat "$f"; fi"#,
            None,
        )?;
        Ok(out.strip_prefix('y').map(str::to_string))
    }

    fn write_shim(t: &ShimTarget, shim: &str) -> Result<()> {
        let script = r#"mkdir -p "$HOME/.local/bin" && cat > "$HOME/.local/bin/toastback" && chmod +x "$HOME/.local/bin/toastback" && printf %s "$HOME""#;
        let home = wsl_sh(t, script, Some(shim))?;
        let who = t
            .user
            .as_deref()
            .map_or(String::new(), |u| format!(" ({u})"));
        outln!("wrote {home}/.local/bin/toastback in {}{who}", t.distro);
        // the user's login shell decides PATH; zsh users set it in .zshenv, bash users in .profile
        let path = wsl_sh(
            t,
            r#"exec "${SHELL:-/bin/sh}" -lc 'printf %s "$PATH"'"#,
            None,
        );
        let bin = format!("{home}/.local/bin");
        match path {
            Ok(path) if path.split(':').any(|p| p == bin || p == "~/.local/bin") => {}
            Ok(_) => outln!(
                "  note: {bin} is not on PATH in {}; add it to your shell profile",
                t.distro
            ),
            Err(e) => outln!(
                "  note: couldn't check whether {bin} is on PATH in {}: {e:#}",
                t.distro
            ),
        }
        Ok(())
    }

    /// Remove the shim from a distro (only a file with our marker), and say how it went. Returns
    /// false if that couldn't be checked.
    fn remove_shim(t: &ShimTarget) -> bool {
        let script = format!(
            r#"f="$HOME/.local/bin/toastback"; if head -n 2 "$f" 2>/dev/null | grep -q '{SHIM_MARKER}'; then rm -f "$f" && echo removed; fi"#
        );
        match wsl_sh(t, &script, None) {
            Ok(out) => {
                if out.contains("removed") {
                    outln!("removed the shim in {}", t.distro);
                } else {
                    outln!("no toastback shim in {}", t.distro);
                }
                true
            }
            Err(e) => {
                errln!("warning: shim in {}: {e:#}", t.distro);
                false
            }
        }
    }

    pub fn uninstall(store: &Store, purge: bool) -> Result<()> {
        let record = readable_record(store);
        let existing = list_distros().ok();
        for t in record.iter().flat_map(|r| &r.shims) {
            if existing.as_ref().is_some_and(|e| !has_distro(e, &t.distro)) {
                outln!(
                    "skipped the shim in {}: the distro no longer exists",
                    t.distro
                );
                continue;
            }
            remove_shim(t);
        }
        if record.as_ref().is_some_and(|r| r.path_added)
            && remove_from_path(&store.bin_dir().to_string_lossy())?
        {
            outln!("removed {} from your user PATH", store.bin_dir().display());
        }
        if let Err(e) = toast::remove_all() {
            errln!("warning: clearing toasts: {e:#}");
        }
        registry::delete_tree(&aumid_key())?;
        registry::delete_tree(PROTOCOL_KEY)?;
        registry::delete_tree(APP_KEY)?;
        outln!("removed the registry keys");
        store.remove_install_record()?;

        let target = if purge {
            store.root().to_path_buf()
        } else {
            store.bin_dir()
        };
        delete_dir(&target)
    }

    /// Delete one of our state dirs; if the running exe lives in it, a detached cmd.exe does it
    /// once we've exited.
    fn delete_dir(target: &PathBuf) -> Result<()> {
        ensure!(
            target.components().any(|c| c.as_os_str() == "toastback"),
            "refusing to delete {}",
            target.display()
        );
        if !target.exists() {
            return Ok(());
        }
        let busy = running_from(target);
        if !busy.is_empty() {
            errln!(
                "warning: {} other toastback process(es) still run from {}:",
                busy.len(),
                target.display()
            );
            for (pid, path) in &busy {
                errln!("  pid {pid}  {}", path.display());
            }
            errln!(
                "  These are callbacks that haven't finished or dialogs that are still open (see\n  \
                 `toastback log`). Their files stay until they exit. Afterwards, remove the rest\n  \
                 from PowerShell with:\n    & \"{}\" uninstall{}",
                busy[0].1.display(),
                if target.ends_with("bin") {
                    ""
                } else {
                    " --purge"
                }
            );
        }
        let what = if busy.is_empty() {
            target.display().to_string()
        } else {
            format!("the files of {} that aren't in use", target.display())
        };
        let me = fs::canonicalize(std::env::current_exe()?)?;
        if me.starts_with(fs::canonicalize(target)?) {
            Command::new(system_exe("cmd.exe")?)
                .raw_arg(format!(
                    "/d /c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"",
                    target.display()
                ))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .context("schedule deleting the install dir")?;
            outln!("{what} will be deleted in a moment");
        } else {
            match fs::remove_dir_all(target) {
                Ok(()) => outln!("deleted {what}"),
                // files in use stay; the rest is gone (remove_dir_all stops at the first failure,
                // so finish with a best-effort pass)
                Err(_) if !busy.is_empty() => {
                    remove_unused(target);
                    outln!("deleted {what}");
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("delete {}", target.display()));
                }
            }
        }
        Ok(())
    }

    /// Delete everything under `dir` that isn't locked, ignoring failures.
    fn remove_unused(dir: &Path) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                remove_unused(&path);
                let _ = fs::remove_dir(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }

    /// Other toastback processes whose exe lives under `dir`: callbacks still running, open dialogs.
    fn running_from(dir: &Path) -> Vec<(u32, PathBuf)> {
        let Ok(dir) = fs::canonicalize(dir) else {
            return Vec::new();
        };
        let me = std::process::id();
        let mut found = Vec::new();
        // SAFETY: the snapshot handle is closed below; entry is a properly sized local struct.
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return found;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut more = Process32FirstW(snap, &mut entry).is_ok();
            while more {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                let ours = name.eq_ignore_ascii_case(GUI_EXE) || name.eq_ignore_ascii_case(CLI_EXE);
                if ours
                    && entry.th32ProcessID != me
                    && let Some(path) = image_path(entry.th32ProcessID)
                    && fs::canonicalize(&path).is_ok_and(|p| p.starts_with(&dir))
                {
                    found.push((entry.th32ProcessID, path));
                }
                more = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        found
    }

    /// Full path of a process's exe, if we may query it.
    fn image_path(pid: u32) -> Option<PathBuf> {
        // SAFETY: the handle is closed before returning; buf and len describe a local buffer.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let res = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(h);
            res.ok()?;
            Some(PathBuf::from(String::from_utf16_lossy(
                &buf[..len as usize],
            )))
        }
    }

    pub fn is_installed(store: &Store) -> Result<bool> {
        Ok(store.install_record()?.is_some() && registry::key_exists(&aumid_key()))
    }

    /// The GUI exe to start detached processes with: the sibling `toastback.exe` when running as
    /// `toastback-cli.exe`, so no console process lingers.
    pub fn gui_exe() -> Result<PathBuf> {
        let me = std::env::current_exe()?;
        let sibling = me.with_file_name(GUI_EXE);
        Ok(if sibling.exists() { sibling } else { me })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_renders() {
        let s = render_shim(
            "0.1.0",
            r"C:\Users\o'neil\AppData\Local\toastback\bin\toastback.exe",
        );
        assert!(s.starts_with("#!/bin/sh\n"));
        assert!(s.lines().nth(1).unwrap().starts_with(SHIM_MARKER));
        assert!(s.contains("toastback 0.1.0)"));
        // systemd services and the like have no WSL_DISTRO_NAME: the shim falls back to wslpath
        assert!(s.contains(r#"distro="${WSL_DISTRO_NAME:-}""#));
        assert!(s.contains("wslpath -w /"));
        assert!(s.contains(
            r"TOASTBACK_WIN_EXE='C:\Users\o'\''neil\AppData\Local\toastback\bin\toastback.exe'"
        ));
        assert!(!s.contains("{{"));
        assert!(!s.contains('\r'));
    }

    #[test]
    fn distro_list_utf16_and_utf8() {
        let utf16: Vec<u8> = "\u{feff}FedoraLinux-44\r\nUbuntu-24.04\r\n\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            parse_distro_list(&utf16),
            ["FedoraLinux-44", "Ubuntu-24.04"]
        );
        assert_eq!(
            parse_distro_list(b"FedoraLinux-44\nUbuntu\n"),
            ["FedoraLinux-44", "Ubuntu"]
        );
        assert!(parse_distro_list(b"").is_empty());
    }

    fn with_shims(shims: &[ShimTarget]) -> InstallRecord {
        InstallRecord {
            shims: shims.to_vec(),
            ..Default::default()
        }
    }

    #[test]
    fn shim_targets_keep_the_calling_user() {
        let ctx = Context {
            distro: Some("Ubuntu".into()),
            user: Some("g".into()),
            cwd: None,
        };
        let list = || {
            Ok(vec![
                "ubuntu".to_string(),
                "Debian".to_string(),
                "docker-desktop".to_string(),
            ])
        };
        let default = || Ok(Some("Debian".to_string()));
        let t = |d: &str, u: Option<&str>| ShimTarget {
            distro: d.into(),
            user: u.map(Into::into),
        };
        let targets = |wsl, ctx: &Context, recorded| {
            shim_targets(wsl, ctx, recorded, list, default)
                .unwrap()
                .targets
        };
        let plan = shim_targets(Some("all"), &ctx, None, list, default).unwrap();
        assert_eq!(plan.targets, [t("ubuntu", Some("g")), t("Debian", None)]);
        assert_eq!(plan.skipped, ["docker-desktop"]);
        // the calling distro needs neither a listing nor the default
        let plan = shim_targets(
            None,
            &ctx,
            None,
            || panic!("no listing needed"),
            || panic!("no default needed"),
        );
        assert_eq!(plan.unwrap().targets, [t("Ubuntu", Some("g"))]);
        assert_eq!(targets(Some("Debian"), &ctx, None), [t("Debian", None)]);
        assert!(targets(Some("none"), &ctx, None).is_empty());
        // named explicitly, Docker's distro is fine
        assert_eq!(
            targets(Some("docker-desktop"), &ctx, None),
            [t("docker-desktop", None)]
        );
        assert!(has_distro(&list().unwrap(), "UBUNTU"));
    }

    #[test]
    fn shim_targets_default_distro() {
        let none = Context::default();
        let in_debian = Context {
            distro: Some("debian".into()),
            user: Some("g".into()),
            cwd: None,
        };
        let no_list = || -> Result<Vec<String>> { panic!("no listing needed") };
        let default = |d: &'static str| move || Ok(Some(d.to_string()));
        let t = |d: &str, u: Option<&str>| ShimTarget {
            distro: d.into(),
            user: u.map(Into::into),
        };
        // a first install from Windows, with or without `--wsl default`
        for wsl in [None, Some("default")] {
            let plan = shim_targets(wsl, &none, None, no_list, default("Debian")).unwrap();
            assert_eq!(plan.targets, [t("Debian", None)]);
        }
        // run from the default distro, it's the calling user's
        let plan = shim_targets(
            Some("default"),
            &in_debian,
            None,
            no_list,
            default("Debian"),
        );
        assert_eq!(plan.unwrap().targets, [t("Debian", Some("g"))]);
        // Docker's distro as the default, or no distro at all: an error naming the way out
        let e = shim_targets(None, &none, None, no_list, default("docker-desktop")).unwrap_err();
        assert!(e.to_string().contains("--wsl NAME"), "{e}");
        let e = shim_targets(None, &none, None, no_list, || Ok(None)).unwrap_err();
        assert!(e.to_string().contains("no default distro"), "{e}");
    }

    #[test]
    fn shim_targets_reuse_the_install_record() {
        let none = Context::default();
        let no_default = || -> Result<Option<String>> { panic!("no default needed") };
        let t = |d: &str, u: Option<&str>| ShimTarget {
            distro: d.into(),
            user: u.map(Into::into),
        };
        let recorded = [t("Ubuntu", Some("g")), t("Removed", None)];
        let list = || Ok(vec!["ubuntu".to_string(), "Debian".to_string()]);
        // users stay as recorded; distros that no longer exist drop out
        let rec = with_shims(&recorded);
        let plan = shim_targets(None, &none, Some(&rec), list, no_default).unwrap();
        assert_eq!(plan.targets, [t("Ubuntu", Some("g"))]);
        // `--wsl none` earlier: stays none, without even asking WSL, even with shims still listed
        let no_list = || -> Result<Vec<String>> { panic!("no listing needed") };
        let declined = InstallRecord {
            no_shims: Some(true),
            ..with_shims(&recorded)
        };
        let plan = shim_targets(None, &none, Some(&declined), no_list, no_default).unwrap();
        assert_eq!(plan, ShimPlan::default());
        // ... and so does a record from before `no_shims` with no shims
        let old = InstallRecord::default();
        let plan = shim_targets(None, &none, Some(&old), no_list, no_default).unwrap();
        assert_eq!(plan, ShimPlan::default());
        // an explicit --wsl wins
        let default = || Ok(Some("Debian".to_string()));
        let plan = shim_targets(Some("default"), &none, Some(&rec), list, default).unwrap();
        assert_eq!(plan.targets, [t("Debian", None)]);
    }

    #[test]
    fn shim_targets_recover_from_an_install_that_wrote_none() {
        let none = Context::default();
        let list = || Ok(vec!["Debian".to_string()]);
        let default = || Ok(Some("Debian".to_string()));
        let debian = [ShimTarget {
            distro: "Debian".into(),
            user: None,
        }];
        // a first install without WSL: not a choice, so a later one uses the default distro
        let nothing_written = InstallRecord {
            no_shims: Some(false),
            ..Default::default()
        };
        let plan = shim_targets(None, &none, Some(&nothing_written), list, default).unwrap();
        assert_eq!(plan.targets, debian);
        // every recorded distro is gone: the same
        let removed = with_shims(&[ShimTarget {
            distro: "Removed".into(),
            user: None,
        }]);
        let plan = shim_targets(None, &none, Some(&removed), list, default).unwrap();
        assert_eq!(plan.targets, debian);
    }

    #[test]
    fn shim_targets_refuse_an_unknown_distro() {
        let no_default = || -> Result<Option<String>> { panic!("no default needed") };
        let list = || Ok(vec!["Ubuntu".to_string()]);
        let none = Context::default();
        let e = shim_targets(Some("Ubunut"), &none, None, list, no_default).unwrap_err();
        assert!(e.to_string().contains("no WSL distro named Ubunut"), "{e}");
        assert!(e.to_string().contains("Ubuntu"), "{e}");
        // without a listing there's no telling: an error too, with nothing changed yet
        let broken = || -> Result<Vec<String>> { anyhow::bail!("stuck") };
        assert!(shim_targets(Some("Ubuntu"), &none, None, broken, no_default).is_err());
    }

    #[test]
    fn default_distro_from_the_verbose_list() {
        let list = "  NAME             STATE      VERSION\n* FedoraLinux-44   Running    2\n  docker-desktop   Stopped    2\n";
        let utf16: Vec<u8> = list.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            parse_default_distro(&utf16).as_deref(),
            Some("FedoraLinux-44")
        );
        assert_eq!(
            parse_default_distro(list.as_bytes()).as_deref(),
            Some("FedoraLinux-44")
        );
        assert_eq!(parse_default_distro(b"  NAME  STATE  VERSION\n"), None);
        assert_eq!(parse_default_distro(b""), None);
        assert!(is_docker_distro("docker-desktop-data"));
        assert!(is_docker_distro("Docker-Desktop"));
        assert!(!is_docker_distro("Ubuntu"));
    }

    #[test]
    fn exes_moved_aside_stay_runnable_and_get_cleaned_up() {
        let old = moved_aside_name("toastback", 123);
        assert_eq!(old, "toastback.old-123.exe");
        assert!(is_stale_copy(&old));
        // left by earlier versions, and by an interrupted copy
        assert!(is_stale_copy("toastback.exe.old-123"));
        assert!(is_stale_copy("toastback-cli.exe.new-123"));
        for current in ["toastback.exe", "toastback-cli.exe", "icon.png"] {
            assert!(!is_stale_copy(current), "{current}");
        }
    }

    #[test]
    fn uninstall_command_quotes_the_path() {
        assert_eq!(
            uninstall_command(r"C:\Users\A B\toastback.exe"),
            r#""C:\Users\A B\toastback.exe" uninstall"#
        );
    }

    #[cfg(windows)]
    #[test]
    fn wait_limited_returns_output_and_kills_on_timeout() {
        let spawn = |args: &[&str]| {
            std::process::Command::new("cmd.exe")
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        };
        let limit = std::time::Duration::from_secs(20);
        let out = imp::wait_limited(spawn(&["/d", "/c", "echo hi"]), limit, "echo").unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hi"));

        let short = std::time::Duration::from_millis(200);
        let started = std::time::Instant::now();
        let e = imp::wait_limited(spawn(&["/d", "/c", "ping -n 30 127.0.0.1"]), short, "ping");
        assert!(e.unwrap_err().to_string().contains("no answer"));
        assert!(started.elapsed() < limit);
    }

    /// A process the child started keeps the pipes open: the child's exit still ends the wait.
    #[cfg(windows)]
    #[test]
    fn wait_limited_does_not_wait_for_what_the_child_started() {
        let child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "start /b ping -n 21 127.0.0.1 & echo hi"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        let limit = std::time::Duration::from_secs(60);
        let out = imp::wait_limited(child, limit, "cmd").unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hi"));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[cfg(windows)]
    #[test]
    fn system_programs_have_a_full_path() {
        let exe = imp::system_exe("hostname.exe").unwrap();
        assert!(exe.is_absolute(), "{}", exe.display());
        assert!(exe.exists(), "{}", exe.display());
        // Windows reports it as `system32` or `System32`
        let dir = exe.parent().unwrap().file_name().unwrap();
        assert!(dir.eq_ignore_ascii_case("system32"), "{}", exe.display());
    }

    #[cfg(windows)]
    #[test]
    fn copy_exe_replaces_only_with_a_complete_copy() {
        let dir = std::env::temp_dir().join(format!("toastback-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (src, dest) = (dir.join("src.exe"), dir.join("dest.exe"));
        std::fs::write(&dest, b"old").unwrap();
        // the source can't be read: the old exe stays
        assert!(imp::copy_exe(&src, &dest).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");
        std::fs::write(&src, b"new").unwrap();
        imp::copy_exe(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        // no temporary copy stays behind
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn user_path_editing() {
        let bin = r"C:\Users\g\AppData\Local\toastback\bin";
        assert_eq!(path_with("", bin).unwrap(), bin);
        assert_eq!(
            path_with(r"%USERPROFILE%\bin;", bin).unwrap(),
            format!(r"%USERPROFILE%\bin;{bin}")
        );
        // already there, in another case or with a trailing backslash
        assert_eq!(
            path_with(r"C:\x;c:\users\g\appdata\local\toastback\bin\", bin),
            None
        );
        assert_eq!(
            path_without(&format!(r"C:\x;{bin};D:\y"), bin).unwrap(),
            r"C:\x;D:\y"
        );
        assert_eq!(path_without(r"C:\x", bin), None);
    }
}
