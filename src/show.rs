// SPDX-License-Identifier: GPL-3.0-or-later
//! `show`: read a spec, merge flags and context, validate, and show it as a toast or dialog.

use std::io::{IsTerminal as _, Read as _};

use anyhow::{Context as _, Result, bail};

use crate::cli::ShowArgs;
use crate::context;
#[cfg(windows)]
use crate::spec::Mode;
use crate::spec::{Context, Spec};
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
            println!("{id}");
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
            std::fs::read_to_string(&path).with_context(|| format!("read {path}"))?
        }
        None if flags.is_empty() => {
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
    Ok(spec)
}

fn read_stdin() -> Result<String> {
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .context("read spec from stdin")?;
    Ok(s)
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
        expires_at: now + spec.expires().map_err(invalid)?.as_secs(),
        done: false,
        icon_path,
        image_path,
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
            let spawned =
                install::gui_exe().and_then(|exe| dialog::spawn_detached(&exe, &["__dialog", &id]));
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
