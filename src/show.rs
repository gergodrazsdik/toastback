// SPDX-License-Identifier: GPL-3.0-or-later
//! `show`: read a spec, merge flags and context, validate, and show it as a toast or dialog.

use std::io::{IsTerminal as _, Read as _};

use anyhow::{Context as _, Result, bail};

use crate::cli::ShowArgs;
#[cfg(windows)]
use crate::spec::Mode;
use crate::spec::{Context, Spec};
use crate::{context, exec};
#[cfg(windows)]
use crate::{dialog, install, store, toast};

/// Exit codes of `show` (PLAN.md §5).
pub const EXIT_INVALID: u8 = 2;
pub const EXIT_SHOW_FAILED: u8 = 3;
pub const EXIT_NOT_INSTALLED: u8 = 4;

/// Read the spec, merge flags and context, validate, show. Prints the id; returns the exit code.
#[cfg(windows)]
pub fn show(store: &store::Store, args: &ShowArgs, ctx: &Context) -> u8 {
    let spec = match load_spec(args, ctx) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("toastback: {e:#}");
            return EXIT_INVALID;
        }
    };
    match install::is_installed(store) {
        Ok(true) => {}
        _ => {
            eprintln!("toastback: not installed; run `toastback.exe install` first");
            return EXIT_NOT_INSTALLED;
        }
    }
    match show_spec(store, spec) {
        Ok(id) => {
            outln!("{id}");
            0
        }
        Err((code, e)) => {
            eprintln!("toastback: {e:#}");
            code
        }
    }
}

pub fn load_spec(args: &ShowArgs, ctx: &Context) -> Result<Spec> {
    let flags = args.flags();
    let json = match args.file.as_deref() {
        Some("-") => read_stdin()?,
        Some(path) => {
            let path = context::to_windows_path(path, ctx)?;
            let bytes = std::fs::read(&path).with_context(|| format!("read {path}"))?;
            decode_text(&bytes).with_context(|| format!("read {path}"))?
        }
        None if spec_from_stdin(&flags) => {
            if std::io::stdin().is_terminal() {
                bail!("no spec: pass a FILE, - for stdin, or -t TITLE (see --help)");
            }
            read_stdin()?
        }
        None => "{}".into(),
    };
    let mut spec = Spec::parse(&json)?;
    spec.apply(&flags);
    spec.context.fill_from(ctx);
    spec.validate()?;
    if spec.context.cwd.is_none()
        && let Ok(dir) = std::env::current_dir()
    {
        resolve_windows_cwds(&mut spec, &dir.to_string_lossy());
    }
    check_runs(&spec)?;
    Ok(spec)
}

/// Started from cmd or PowerShell (no shim, so no caller directory), a relative `cwd` of a
/// Windows-side callback means the directory `toastback` runs in. The click handler runs
/// elsewhere, so make it absolute now. (`wsl` callbacks need a Linux directory: still an error.)
fn resolve_windows_cwds(spec: &mut Spec, dir: &str) {
    let runs = spec.buttons.iter_mut().filter_map(|b| b.run.as_mut());
    for run in runs.chain(spec.default.as_mut()) {
        let windows_side = run.kind().is_ok_and(|k| {
            !matches!(
                k,
                crate::spec::RunKind::WslShell(_) | crate::spec::RunKind::WslArgv(_)
            )
        });
        if let Some(c) = run.cwd.as_mut()
            && windows_side
            && !context::is_windows_path(c)
            && !c.starts_with(['/', '~'])
        {
            *c = format!(r"{}\{}", dir.trim_end_matches('\\'), c.replace('/', "\\"));
        }
    }
}

/// What would otherwise only fail at click time: a relative cwd without a caller directory, ...
fn check_runs(spec: &Spec) -> Result<()> {
    let buttons = spec.buttons.iter().filter_map(|b| {
        b.run
            .as_ref()
            .map(|r| (format!("button {:?}: run", b.id), r))
    });
    let default = spec.default.as_ref().map(|r| ("default".to_string(), r));
    for (what, run) in buttons.chain(default) {
        exec::check(run, &spec.context).context(what)?;
    }
    Ok(())
}

/// Without a FILE: -t or -m make a spec on their own; other flags (--mode, --id, -i, ...) only
/// modify the one from stdin.
fn spec_from_stdin(flags: &crate::spec::QuickFlags) -> bool {
    flags.title.is_none() && flags.body.is_none()
}

fn read_stdin() -> Result<String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .context("read spec from stdin")?;
    decode_text(&bytes).context("read spec from stdin")
}

/// Spec text as written by Windows tools too: UTF-8 with or without a BOM, or UTF-16 with one
/// (Windows PowerShell's `Set-Content -Encoding UTF8`, `Out-File` and `>`).
pub fn decode_text(bytes: &[u8]) -> Result<String> {
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec()).context("invalid UTF-8"),
        [0xFF, 0xFE, rest @ ..] => String::from_utf16le(rest).context("invalid UTF-16"),
        [0xFE, 0xFF, rest @ ..] => String::from_utf16be(rest).context("invalid UTF-16"),
        _ => String::from_utf8(bytes.to_vec()).context("invalid UTF-8 (save the file as UTF-8)"),
    }
}

/// Show a validated spec: id, token, cached images, pending file, toast. Errors carry an exit code.
#[cfg(windows)]
pub fn show_spec(store: &store::Store, mut spec: Spec) -> Result<String, (u8, anyhow::Error)> {
    use crate::store::{Pending, new_id, new_token, unix_now};
    use serde_json::json;

    let invalid = |e: anyhow::Error| (EXIT_INVALID, e);
    let failed = |e: anyhow::Error| (EXIT_SHOW_FAILED, e);

    let id = match &spec.id {
        Some(id) => id.clone(),
        None => new_id().map_err(failed)?,
    };
    spec.id = Some(id.clone());
    let cache = |kind: &str, src: &Option<String>| -> Result<Option<String>> {
        src.as_deref()
            .map(|p| {
                let win = context::to_windows_path(p, &spec.context).context(kind.to_string())?;
                Ok(store
                    .cache_image(&id, kind, &win)?
                    .to_string_lossy()
                    .into_owned())
            })
            .transpose()
    };
    let icon_path = cache("icon", &spec.icon).map_err(invalid)?;
    let image_path = cache("image", &spec.image).map_err(invalid)?;

    let now = unix_now();
    let pending = Pending {
        token: new_token().map_err(failed)?,
        created: now,
        expires_at: now + spec.lifetime().map_err(invalid)?.as_secs(),
        done: false,
        icon_path,
        image_path,
        dialog_open: false,
        spec,
    };
    store.save_pending(&pending).map_err(failed)?;

    match pending.spec.mode {
        Mode::Toast => {
            let images = toast::Images {
                icon: pending.icon_path.as_deref(),
                image: pending.image_path.as_deref(),
            };
            let xml = toast::build_xml(&pending.spec, &id, &pending.token, &images);
            toast::show(&xml, &id, pending.expires_at).map_err(failed)?;
        }
        Mode::Dialog => {
            // a separate process shows the dialog, so the caller returns right away
            let spawned = install::gui_exe()
                .and_then(|exe| dialog::spawn_detached(&exe, &["__dialog", "--", &id]));
            if let Err(e) = spawned {
                let _ = store.remove_pending(&id);
                return Err(failed(e));
            }
        }
    }
    let _ = store.log(
        "shown",
        json!({ "id": id, "mode": pending.spec.mode, "title": pending.spec.title }),
    );
    Ok(id)
}

/// `validate`: parse and check one spec file, including that its images can be read.
pub fn validate_file(file: &str, ctx: &Context) -> Result<()> {
    let args = ShowArgs {
        file: Some(file.to_string()),
        ..Default::default()
    };
    let spec = load_spec(&args, ctx)?;
    for (kind, path) in [("icon", &spec.icon), ("image", &spec.image)] {
        if let Some(p) = path {
            let win = context::to_windows_path(p, &spec.context).context(kind)?;
            std::fs::metadata(&win).with_context(|| format!("{kind}: cannot read {win}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_windows_encodings() {
        let json = r#"{"title": "Jörg"}"#;
        let utf16le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(json.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let utf16be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain(json.encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        let bom: Vec<u8> = [0xEF, 0xBB, 0xBF].into_iter().chain(json.bytes()).collect();
        for bytes in [json.as_bytes(), &bom, &utf16le, &utf16be] {
            assert_eq!(decode_text(bytes).unwrap(), json);
        }
        assert!(decode_text(&[0xFF, 0xFE, 0x41]).is_err());
        assert!(decode_text(&[0xC3]).is_err());
    }

    #[test]
    fn only_title_and_body_replace_stdin() {
        use crate::spec::{Mode, QuickFlags};
        assert!(spec_from_stdin(&QuickFlags::default()));
        assert!(spec_from_stdin(&QuickFlags {
            mode: Some(Mode::Dialog),
            id: Some("build-42".into()),
            icon: Some("i.png".into()),
            details: Some("d".into()),
            ..Default::default()
        }));
        assert!(!spec_from_stdin(&QuickFlags {
            title: Some("t".into()),
            ..Default::default()
        }));
        assert!(!spec_from_stdin(&QuickFlags {
            body: Some("b".into()),
            ..Default::default()
        }));
    }

    #[test]
    fn relative_windows_cwds_resolve_against_the_current_dir() {
        let mut spec = Spec::parse(
            r#"{"title": "t", "default": {"pwsh": "x", "cwd": "sub/dir"},
                "buttons": [{"id": "w", "label": "W", "run": {"wsl": "x", "cwd": "sub"}},
                            {"id": "a", "label": "A", "run": {"cmd": "x", "cwd": "D:\\abs"}}]}"#,
        )
        .unwrap();
        resolve_windows_cwds(&mut spec, r"C:\work\");
        assert_eq!(
            spec.default.unwrap().cwd.as_deref(),
            Some(r"C:\work\sub\dir")
        );
        assert_eq!(
            spec.buttons[0].run.as_ref().unwrap().cwd.as_deref(),
            Some("sub")
        );
        assert_eq!(
            spec.buttons[1].run.as_ref().unwrap().cwd.as_deref(),
            Some(r"D:\abs")
        );
    }

    #[test]
    fn click_time_errors_surface_at_show_time() {
        let spec = Spec::parse(
            r#"{"title": "t", "context": {"distro": "D"},
                "buttons": [{"id": "b", "label": "B", "run": {"cmd": "dir", "cwd": "/home/g"}}]}"#,
        )
        .unwrap();
        let e = format!("{:#}", check_runs(&spec).unwrap_err());
        assert!(e.contains(r#"button "b": run"#) && e.contains("UNC"), "{e}");
    }
}
