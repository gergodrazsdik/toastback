// SPDX-License-Identifier: GPL-3.0-or-later
//! `install`, `uninstall`, the WSL shim and the user PATH entry.

const SHIM_TEMPLATE: &str = include_str!("../res/shim.sh.tmpl");
/// First-line marker: `uninstall` only deletes files that carry it.
pub const SHIM_MARKER: &str = "# toastback WSL shim";
pub const GUI_EXE: &str = "toastback.exe";
pub const CLI_EXE: &str = "toastback-cli.exe";

/// Render the WSL shim for an installed exe at `win_exe` (a Windows path).
pub fn render_shim(version: &str, win_exe: &str) -> String {
    // the path sits in single quotes: close, escaped quote, reopen
    let quoted = win_exe.replace('\'', r"'\''");
    SHIM_TEMPLATE
        .replace("{{VERSION}}", version)
        .replace("{{WIN_EXE_PATH}}", &quoted)
}

/// The protocol handler command registered for `toastback://`.
pub fn protocol_command(exe: &str) -> String {
    format!("\"{exe}\" activate \"%1\"")
}

/// Parse `wsl.exe -l -q` output: UTF-16LE (with BOM/NULs) by default, UTF-8 with `WSL_UTF8=1`.
pub fn parse_distro_list(bytes: &[u8]) -> Vec<String> {
    let text = if bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).any(|&b| b == 0) {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0'))
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn same_dir(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\')
        .eq_ignore_ascii_case(b.trim_end_matches('\\'))
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
    use std::process::{Command, Stdio};

    use anyhow::{Context as _, Result, bail, ensure};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    use windows::core::w;

    use super::{
        CLI_EXE, GUI_EXE, SHIM_MARKER, parse_distro_list, path_with, path_without,
        protocol_command, render_shim,
    };
    use crate::registry::{self, Kind};
    use crate::spec::Context;
    use crate::store::{ShimTarget, Store, timestamp};
    use crate::toast::{self, AUMID};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    pub const PROTOCOL_KEY: &str = r"Software\Classes\toastback";
    pub const PROTOCOL_COMMAND_KEY: &str = r"Software\Classes\toastback\shell\open\command";
    pub const ENV_KEY: &str = "Environment";

    pub fn aumid_key() -> String {
        format!(r"Software\Classes\AppUserModelId\{AUMID}")
    }

    pub struct Options<'a> {
        /// A distro name, `all`, `none`, or `None` for "the calling distro, else all".
        pub wsl: Option<&'a str>,
        pub add_to_path: bool,
    }

    pub fn install(store: &Store, opts: &Options, ctx: &Context, version: &str) -> Result<()> {
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
        if cli.exists() {
            copy_exe(&cli, &store.bin_dir().join(CLI_EXE))?;
        }
        println!(
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
        println!("registered the toastback:// handler and the notification app id");

        let mut record = store.install_record()?.unwrap_or_default();
        if opts.add_to_path {
            if add_to_path(&store.bin_dir().to_string_lossy())? {
                println!(
                    "added {} to your user PATH (new terminals see it)",
                    store.bin_dir().display()
                );
            } else {
                println!("{} is already on your user PATH", store.bin_dir().display());
            }
            record.path_added = true;
        }

        let shim = render_shim(version, &exe.to_string_lossy());
        for t in &shim_targets(opts.wsl, ctx)? {
            match write_shim(t, &shim) {
                Ok(()) => {
                    if !record.shims.contains(t) {
                        record.shims.push(t.clone());
                    }
                }
                Err(e) => eprintln!("warning: shim for {}: {e:#}", t.distro),
            }
        }
        record.version = version.into();
        record.installed_at = timestamp(std::time::SystemTime::now());
        record.exe = exe.to_string_lossy().into_owned();
        store.save_install_record(&record)?;
        Ok(())
    }

    /// `*.old-<pid>` files left over from replacing an exe that was running.
    fn remove_stale_copies(bin: &Path) {
        for entry in fs::read_dir(bin).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().contains(".exe.old") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// Copy an exe as fresh bytes, so the download marker (Zone.Identifier) doesn't come along.
    fn copy_exe(src: &Path, dest: &Path) -> Result<()> {
        if dest.exists() && fs::canonicalize(src)? == fs::canonicalize(dest)? {
            return Ok(()); // installing from the installed copy
        }
        let bytes = fs::read(src).with_context(|| format!("read {}", src.display()))?;
        if dest.exists() && fs::remove_file(dest).is_err() {
            // in use (a callback is running): a running exe can be renamed, not deleted
            let old = dest.with_extension(format!("exe.old-{}", std::process::id()));
            fs::rename(dest, &old)
                .with_context(|| format!("move the running {} aside", dest.display()))?;
        }
        fs::write(dest, bytes).with_context(|| format!("write {}", dest.display()))
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

    fn shim_targets(wsl: Option<&str>, ctx: &Context) -> Result<Vec<ShimTarget>> {
        let all = || -> Result<Vec<ShimTarget>> {
            Ok(list_distros()?
                .into_iter()
                .map(|distro| ShimTarget { distro, user: None })
                .collect())
        };
        Ok(match wsl {
            Some("none") => Vec::new(),
            Some("all") => all()?,
            Some(d) => vec![ShimTarget {
                distro: d.into(),
                // the calling distro gets the calling user; others their default user
                user: ctx
                    .user
                    .clone()
                    .filter(|_| ctx.distro.as_deref() == Some(d)),
            }],
            None => match &ctx.distro {
                Some(d) => vec![ShimTarget {
                    distro: d.clone(),
                    user: ctx.user.clone(),
                }],
                None => all()?,
            },
        })
    }

    pub fn list_distros() -> Result<Vec<String>> {
        let out = Command::new("wsl.exe")
            .args(["-l", "-q"])
            .env("WSL_UTF8", "1")
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .context("run wsl.exe -l -q")?;
        ensure!(out.status.success(), "wsl.exe -l -q failed");
        Ok(parse_distro_list(&out.stdout))
    }

    /// Run `sh -c script` in a distro, optionally feeding `stdin`. Returns stdout.
    pub fn wsl_sh(t: &ShimTarget, script: &str, stdin: Option<&str>) -> Result<String> {
        let mut c = Command::new("wsl.exe");
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
        if let Some(input) = stdin {
            child.stdin.take().unwrap().write_all(input.as_bytes())?;
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!(
                "exit {:?}: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
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
        println!("wrote {home}/.local/bin/toastback in {}{who}", t.distro);
        // the user's login shell decides PATH; zsh users set it in .zshenv, bash users in .profile
        let path = wsl_sh(
            t,
            r#"exec "${SHELL:-/bin/sh}" -lc 'printf %s "$PATH"'"#,
            None,
        )
        .unwrap_or_default();
        let bin = format!("{home}/.local/bin");
        if !path.split(':').any(|p| p == bin || p == "~/.local/bin") {
            println!(
                "  note: {bin} is not on PATH in {}; add it to your shell profile",
                t.distro
            );
        }
        Ok(())
    }

    pub fn uninstall(store: &Store, purge: bool) -> Result<()> {
        let record = store.install_record()?;
        let script = format!(
            r#"f="$HOME/.local/bin/toastback"; if head -n 2 "$f" 2>/dev/null | grep -q '{SHIM_MARKER}'; then rm -f "$f" && echo removed; fi"#
        );
        for t in record.iter().flat_map(|r| &r.shims) {
            match wsl_sh(t, &script, None) {
                Ok(out) if out.contains("removed") => println!("removed the shim in {}", t.distro),
                Ok(_) => println!("no toastback shim in {}", t.distro),
                Err(e) => eprintln!("warning: shim in {}: {e:#}", t.distro),
            }
        }
        if record.as_ref().is_some_and(|r| r.path_added)
            && remove_from_path(&store.bin_dir().to_string_lossy())?
        {
            println!("removed {} from your user PATH", store.bin_dir().display());
        }
        if let Err(e) = toast::remove_all() {
            eprintln!("warning: clearing toasts: {e:#}");
        }
        registry::delete_tree(&aumid_key())?;
        registry::delete_tree(PROTOCOL_KEY)?;
        println!("removed the registry keys");
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
        let me = fs::canonicalize(std::env::current_exe()?)?;
        if me.starts_with(fs::canonicalize(target)?) {
            Command::new("cmd.exe")
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
            println!("{} will be deleted in a moment", target.display());
        } else {
            fs::remove_dir_all(target).with_context(|| format!("delete {}", target.display()))?;
            println!("deleted {}", target.display());
        }
        Ok(())
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
        assert!(s.starts_with("#!/usr/bin/env bash\n"));
        assert!(s.lines().nth(1).unwrap().starts_with(SHIM_MARKER));
        assert!(s.contains("toastback 0.1.0)"));
        assert!(s.contains(
            r"TOASTBACK_WIN_EXE='C:\Users\o'\''neil\AppData\Local\toastback\bin\toastback.exe'"
        ));
        assert!(!s.contains("{{"));
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
