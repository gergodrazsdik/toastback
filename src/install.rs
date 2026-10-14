// SPDX-License-Identifier: GPL-3.0-or-later
//! `install`, `uninstall` and the WSL shim.

const SHIM_TEMPLATE: &str = include_str!("../res/shim.sh.tmpl");
/// First-line marker: `uninstall` only deletes files that carry it.
pub const SHIM_MARKER: &str = "# toastback WSL shim";

/// Render the WSL shim for an installed exe at `win_exe` (a Windows path).
pub fn render_shim(version: &str, win_exe: &str) -> String {
    // the path sits in single quotes: close, escaped quote, reopen
    let quoted = win_exe.replace('\'', r"'\''");
    SHIM_TEMPLATE
        .replace("{{VERSION}}", version)
        .replace("{{WIN_EXE_PATH}}", &quoted)
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

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use std::fs;
    use std::io::Write as _;
    use std::os::windows::process::CommandExt as _;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use anyhow::{Context as _, Result, bail, ensure};
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegSetValueExW,
    };
    use windows::core::{HSTRING, PCWSTR};

    use super::{SHIM_MARKER, parse_distro_list, render_shim};
    use crate::spec::Context;
    use crate::store::{ShimTarget, Store, timestamp};
    use crate::toast::{self, AUMID};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const PROTOCOL_KEY: &str = r"Software\Classes\toastback";

    fn aumid_key() -> String {
        format!(r"Software\Classes\AppUserModelId\{AUMID}")
    }

    /// `--wsl`: a distro name, `all`, `none`, or `None` for "the calling distro, else all".
    pub fn install(store: &Store, wsl: Option<&str>, ctx: &Context, version: &str) -> Result<()> {
        let exe = store.installed_exe();
        copy_self(store, &exe)?;
        let icon = store.bin_dir().join("icon.png");
        fs::write(&icon, include_bytes!("../res/icon.png"))?;

        set_value(&aumid_key(), "DisplayName", "toastback")?;
        set_value(&aumid_key(), "IconUri", &icon.to_string_lossy())?;
        set_value(PROTOCOL_KEY, "", "URL:toastback")?;
        set_value(PROTOCOL_KEY, "URL Protocol", "")?;
        set_value(
            &format!(r"{PROTOCOL_KEY}\shell\open\command"),
            "",
            &format!("\"{}\" activate \"%1\"", exe.display()),
        )?;
        println!("registered the toastback:// handler and the notification app id");

        let mut record = store.install_record()?.unwrap_or_default();
        let targets = shim_targets(wsl, ctx)?;
        let shim = render_shim(version, &exe.to_string_lossy());
        for t in &targets {
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

    /// Copy the running exe into `bin\`. Written as fresh bytes, so the download marker
    /// (Zone.Identifier) of a browser download doesn't come along.
    fn copy_self(store: &Store, dest: &Path) -> Result<()> {
        let bin = store.bin_dir();
        fs::create_dir_all(&bin)?;
        // leftovers from replacing a locked exe
        for entry in fs::read_dir(&bin)?.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("toastback.exe.old")
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        let me = std::env::current_exe()?;
        if dest.exists() && fs::canonicalize(&me)? == fs::canonicalize(dest)? {
            println!("running the installed copy: {}", dest.display());
            return Ok(());
        }
        let bytes = fs::read(&me).with_context(|| format!("read {}", me.display()))?;
        if dest.exists() && fs::remove_file(dest).is_err() {
            // in use (a callback is running): a running exe can be renamed, not deleted
            let old = dest.with_extension(format!("exe.old-{}", std::process::id()));
            fs::rename(dest, &old).context("move the running exe aside")?;
        }
        fs::write(dest, bytes).with_context(|| format!("write {}", dest.display()))?;
        println!(
            "installed {} (written fresh, so no \"downloaded from the internet\" marker carries over)",
            dest.display()
        );
        Ok(())
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

    fn list_distros() -> Result<Vec<String>> {
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
    fn wsl_sh(t: &ShimTarget, script: &str, stdin: Option<&str>) -> Result<String> {
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
        if let Err(e) = toast::remove_all() {
            eprintln!("warning: clearing toasts: {e:#}");
        }
        delete_tree(&aumid_key())?;
        delete_tree(PROTOCOL_KEY)?;
        println!("removed the registry keys");
        store.remove_install_record()?;

        let target = if purge {
            store.root().to_path_buf()
        } else {
            store.bin_dir()
        };
        ensure!(
            target.components().any(|c| c.as_os_str() == "toastback"),
            "refusing to delete {}",
            target.display()
        );
        if !target.exists() {
            return Ok(());
        }
        let me = fs::canonicalize(std::env::current_exe()?)?;
        if me.starts_with(fs::canonicalize(&target)?) {
            // can't delete the running exe: let a detached cmd.exe do it once we've exited
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
            fs::remove_dir_all(&target).with_context(|| format!("delete {}", target.display()))?;
            println!("deleted {}", target.display());
        }
        Ok(())
    }

    pub fn is_installed(store: &Store) -> Result<bool> {
        Ok(store.install_record()?.is_some() && key_exists(&aumid_key()))
    }

    fn key_exists(subkey: &str) -> bool {
        let mut key = HKEY::default();
        // SAFETY: valid predefined root, NUL-terminated string, out-pointer to a local HKEY.
        let err = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(subkey),
                None,
                KEY_READ,
                &mut key,
            )
        };
        if err.is_ok() {
            // SAFETY: key was just opened.
            unsafe {
                let _ = RegCloseKey(key);
            }
        }
        err.is_ok()
    }

    fn set_value(subkey: &str, name: &str, value: &str) -> Result<()> {
        let mut key = HKEY::default();
        // SAFETY: valid predefined root, NUL-terminated strings, out-pointer to a local HKEY.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(subkey),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
            .ok()
            .with_context(|| format!(r"create HKCU\{subkey}"))?;
        }
        let data: Vec<u8> = value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        // SAFETY: key was opened above; data is a NUL-terminated UTF-16 buffer.
        let res = unsafe { RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(&data)) };
        // SAFETY: key is a valid open handle.
        unsafe {
            let _ = RegCloseKey(key);
        }
        res.ok().with_context(|| format!(r"set HKCU\{subkey}"))
    }

    fn delete_tree(subkey: &str) -> Result<()> {
        // SAFETY: valid predefined root and a NUL-terminated string.
        let err = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(subkey)) };
        if err.is_ok() || err == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            err.ok().with_context(|| format!(r"delete HKCU\{subkey}"))
        }
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
}
