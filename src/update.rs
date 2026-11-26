// SPDX-License-Identifier: GPL-3.0-or-later
//! `toastback update ID`: change a shown toast's progress bar in place, without a new banner or
//! sound (ADR 0019). Showing again with the same `--id` stays the way to replace a toast.

use anyhow::{Result, bail};

use crate::spec::{Progress, ProgressValue};

/// What `update` changes; `None` keeps the current value.
#[derive(Debug, Default)]
pub struct Change {
    pub value: Option<ProgressValue>,
    pub status: Option<String>,
    pub title: Option<String>,
    pub label: Option<String>,
}

/// The progress after `change`. Windows can only update what the toast was shown with, so a title
/// or label it didn't have is an error (show it again with one).
pub fn apply(current: &Progress, change: &Change) -> Result<Progress> {
    if change.value.is_none()
        && change.status.is_none()
        && change.title.is_none()
        && change.label.is_none()
    {
        bail!("nothing to update: pass --progress, --status, --title or --label");
    }
    if change.title.is_some() && current.title.is_none() {
        bail!("--title: the toast was shown without a progress title; show it again with one");
    }
    if change.label.is_some() && current.label.is_none() {
        bail!("--label: the toast was shown without a progress label; show it again with one");
    }
    let mut next = current.clone();
    if let Some(v) = change.value {
        next.value = v;
    }
    for (slot, new) in [
        (&mut next.status, &change.status),
        (&mut next.title, &change.title),
        (&mut next.label, &change.label),
    ] {
        if new.is_some() {
            slot.clone_from(new);
        }
    }
    Ok(next)
}

/// Update notification `id`. Exit 0 updated, 2 can't be updated that way, 3 Windows failed, 5 no
/// longer shown (answered, dismissed, removed or expired).
#[cfg(windows)]
pub fn run(store: &crate::store::Store, id: &str, change: &Change) -> u8 {
    use serde_json::json;

    use crate::show::{EXIT_INVALID, EXIT_SHOW_FAILED};
    use crate::spec::Mode;
    use crate::toast;
    use crate::wait::EXIT_CLOSED;

    let pending = match store.load_pending(id) {
        Ok(Some(p)) if !p.done => p,
        Ok(_) => {
            errln!("toastback: {id} is no longer shown");
            return EXIT_CLOSED;
        }
        Err(e) => {
            errln!("toastback: {e:#}");
            return EXIT_SHOW_FAILED;
        }
    };
    let current = match (&pending.spec.mode, &pending.spec.progress) {
        (Mode::Toast, Some(p)) => p,
        (Mode::Dialog, _) => {
            errln!("toastback: {id} is a dialog; only toasts have a progress bar");
            return EXIT_INVALID;
        }
        (_, None) => {
            errln!(
                "toastback: {id} was shown without a progress bar; show it again with --progress"
            );
            return EXIT_INVALID;
        }
    };
    let next = match apply(current, change) {
        Ok(p) => p,
        Err(e) => {
            errln!("toastback: {e:#}");
            return EXIT_INVALID;
        }
    };
    match toast::update(id, &toast::progress_data(&next)) {
        Ok(true) => {
            if let Err(e) = store.set_progress(id, &pending.token, &next) {
                errln!("warning: {e:#}");
            }
            let _ = store.log(
                "updated",
                json!({ "id": id, "action": next.value.to_xml_value() }),
            );
            0
        }
        Ok(false) => {
            errln!("toastback: {id} is no longer shown");
            EXIT_CLOSED
        }
        Err(e) => {
            errln!("toastback: {e:#}");
            EXIT_SHOW_FAILED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Indeterminate;

    fn shown(title: bool, label: bool) -> Progress {
        Progress {
            value: ProgressValue::Fraction(0.1),
            status: Some("Copying".into()),
            title: title.then(|| "backup.tar".into()),
            label: label.then(|| "1 of 10".into()),
        }
    }

    #[test]
    fn changes_what_is_given() {
        let next = apply(
            &shown(true, true),
            &Change {
                value: Some(ProgressValue::Fraction(0.5)),
                label: Some("5 of 10".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(next.value, ProgressValue::Fraction(0.5));
        assert_eq!(next.label.as_deref(), Some("5 of 10"));
        assert_eq!(next.status.as_deref(), Some("Copying"));
        assert_eq!(next.title.as_deref(), Some("backup.tar"));

        let next = apply(
            &shown(false, false),
            &Change {
                value: Some(ProgressValue::Indeterminate(Indeterminate::Indeterminate)),
                status: Some("Verifying".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(next.status.as_deref(), Some("Verifying"));
    }

    #[test]
    fn refuses_what_the_toast_cant_show() {
        let e = |p: &Progress, c: Change| format!("{:#}", apply(p, &c).unwrap_err());
        assert!(e(&shown(true, true), Change::default()).contains("nothing to update"));
        assert!(
            e(
                &shown(false, true),
                Change {
                    title: Some("t".into()),
                    ..Default::default()
                }
            )
            .contains("without a progress title")
        );
        assert!(
            e(
                &shown(true, false),
                Change {
                    label: Some("l".into()),
                    ..Default::default()
                }
            )
            .contains("without a progress label")
        );
    }
}
