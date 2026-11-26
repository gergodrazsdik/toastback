// SPDX-License-Identifier: GPL-3.0-or-later
//! `--wait` and `toastback wait ID`: wait until a notification is answered (ADR 0018).
//!
//! No channel between processes: the click handler records the answer in the pending file when it
//! claims the click, and the waiting process polls that file. A toast the user dismissed leaves
//! the notification center, which the waiting process asks Windows about.

use std::time::Duration;

use crate::spec::Mode;
use crate::store::{Pending, deadline, token_matches};

/// Closed without a choice: dismissed, a dialog closed, removed, or shown again under its id.
pub const EXIT_CLOSED: u8 = 5;
/// `--timeout` passed, or the notification expired.
pub const EXIT_TIMED_OUT: u8 = 6;
/// Ctrl-C, as shells report SIGINT (128 + 2).
pub const EXIT_INTERRUPTED: u8 = 130;

/// How often the pending file is read.
const POLL: Duration = Duration::from_millis(200);
/// Clicking a toast removes it from the notification center before the click process has
/// claimed it, so a toast counts as dismissed only once it has been gone this long, unclaimed.
pub const DISMISS_GRACE: Duration = Duration::from_secs(5);

/// The WSL process that started the wait (the shim, `exec`ed into WSL's interop relay). Ctrl-C
/// ends it but never reaches the Windows process, so the wait watches it instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub distro: String,
    pub pid: u32,
}

impl Caller {
    /// Its `/proc` entry, as Windows sees it.
    pub fn proc_path(&self) -> String {
        format!(r"\\wsl.localhost\{}\proc\{}", self.distro, self.pid)
    }
}

/// How waiting ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Clicked: a button id, or `__body`.
    Answer(String),
    /// Ended without a choice; the reason is for the log.
    Closed(&'static str),
    TimedOut,
    Interrupted,
}

impl Outcome {
    pub fn exit_code(&self) -> u8 {
        match self {
            Outcome::Answer(_) => 0,
            Outcome::Closed(_) => EXIT_CLOSED,
            Outcome::TimedOut => EXIT_TIMED_OUT,
            Outcome::Interrupted => EXIT_INTERRUPTED,
        }
    }
}

/// One look at the notification shown with `token`: `Some` once waiting is over. `gone`: the toast
/// has been missing from the notification center for [`DISMISS_GRACE`].
pub fn check(p: Option<&Pending>, token: &str, now: u64, gone: bool) -> Option<Outcome> {
    let Some(p) = p else {
        return Some(Outcome::Closed("removed"));
    };
    if !token_matches(&p.token, token) {
        return Some(Outcome::Closed("shown again"));
    }
    if p.done {
        return Some(match &p.answer {
            Some(a) => Outcome::Answer(a.clone()),
            None => Outcome::Closed("closed without a choice"),
        });
    }
    if now > deadline(p.expires_at, p.dialog_open) {
        return Some(Outcome::TimedOut);
    }
    // a details dialog opened from the toast is still open: its choice is the answer
    if p.spec.mode == Mode::Toast && gone && !p.dialog_open {
        return Some(Outcome::Closed("dismissed"));
    }
    None
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use serde_json::json;

    use super::{Caller, DISMISS_GRACE, Outcome, POLL, check};
    use crate::spec::Mode;
    use crate::store::{Store, unix_now};
    use crate::{console, toast};

    /// How often the caller's `/proc` entry is checked (a round trip to the distro).
    const CALLER_EVERY: u32 = 5;

    /// Is the caller gone? Only a definite "not found" counts: a share that's slow or briefly
    /// unreachable isn't a Ctrl-C.
    fn caller_gone(c: &Caller) -> bool {
        matches!(std::fs::metadata(c.proc_path()), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    }

    /// Wait for notification `id`. `None`: there's no such notification.
    pub fn wait(
        store: &Store,
        id: &str,
        limit: Option<Duration>,
        caller: Option<&Caller>,
    ) -> Option<Outcome> {
        let first = store.load_pending(id).ok().flatten()?;
        let token = first.token.clone();
        let is_toast = first.spec.mode == Mode::Toast;
        let interrupted = console::catch_ctrl_c();
        let start = Instant::now();
        let mut gone_since: Option<Instant> = None;
        let mut round = 0u32;
        let outcome = loop {
            round = round.wrapping_add(1);
            if interrupted.load(Ordering::Relaxed)
                || (round.is_multiple_of(CALLER_EVERY) && caller.is_some_and(caller_gone))
            {
                break Outcome::Interrupted;
            }
            // a file being replaced can't be read for a moment: look again next time
            if let Ok(pending) = store.load_pending(id) {
                if is_toast && pending.as_ref().is_some_and(|p| !p.done) {
                    match toast::in_center(id) {
                        Ok(true) => gone_since = None,
                        Ok(false) => {
                            gone_since.get_or_insert_with(Instant::now);
                        }
                        Err(_) => {}
                    }
                }
                let gone = gone_since.is_some_and(|t| t.elapsed() >= DISMISS_GRACE);
                if let Some(o) = check(pending.as_ref(), &token, unix_now(), gone) {
                    break o;
                }
            }
            if limit.is_some_and(|l| start.elapsed() >= l) {
                break Outcome::TimedOut;
            }
            std::thread::sleep(POLL);
        };
        let (event, detail) = match &outcome {
            Outcome::Answer(a) => ("wait_answered", a.as_str()),
            Outcome::Closed(why) => ("wait_closed", *why),
            Outcome::TimedOut => ("wait_timed_out", ""),
            Outcome::Interrupted => ("wait_interrupted", ""),
        };
        let _ = store.log(event, json!({ "id": id, "action": detail }));
        Some(outcome)
    }

    /// Print the answer, if any; returns the exit code.
    pub fn report(outcome: &Outcome) -> u8 {
        if let Outcome::Answer(a) = outcome {
            outln!("{a}");
        }
        outcome.exit_code()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Spec;

    fn pending(mode: Mode) -> Pending {
        Pending {
            spec: Spec {
                title: "t".into(),
                mode,
                ..Default::default()
            },
            token: "tok".into(),
            created: 0,
            expires_at: 100,
            done: false,
            icon_path: None,
            image_path: None,
            dialog_open: false,
            answer: None,
        }
    }

    #[test]
    fn outcomes() {
        let p = pending(Mode::Toast);
        assert_eq!(check(Some(&p), "tok", 50, false), None);
        assert_eq!(
            check(None, "tok", 50, false),
            Some(Outcome::Closed("removed"))
        );
        assert_eq!(
            check(Some(&p), "other", 50, false),
            Some(Outcome::Closed("shown again"))
        );
        assert_eq!(check(Some(&p), "tok", 101, false), Some(Outcome::TimedOut));
        assert_eq!(
            check(Some(&p), "tok", 50, true),
            Some(Outcome::Closed("dismissed"))
        );

        let mut done = p.clone();
        done.done = true;
        done.answer = Some("deploy".into());
        // an answer wins over everything that happened after it
        assert_eq!(
            check(Some(&done), "tok", 500, true),
            Some(Outcome::Answer("deploy".into()))
        );
        done.answer = None;
        assert_eq!(
            check(Some(&done), "tok", 50, false),
            Some(Outcome::Closed("closed without a choice"))
        );

        // a details dialog is open: not dismissed, and its choice counts past expiry for a while
        let mut asking = p.clone();
        asking.dialog_open = true;
        assert_eq!(check(Some(&asking), "tok", 101, true), None);
        // dialogs aren't in the notification center
        assert_eq!(check(Some(&pending(Mode::Dialog)), "tok", 50, true), None);
    }

    #[test]
    fn caller_path() {
        let c = Caller {
            distro: "Ubuntu-24.04".into(),
            pid: 4242,
        };
        assert_eq!(c.proc_path(), r"\\wsl.localhost\Ubuntu-24.04\proc\4242");
    }

    #[test]
    fn exit_codes() {
        assert_eq!(Outcome::Answer("a".into()).exit_code(), 0);
        assert_eq!(Outcome::Closed("dismissed").exit_code(), EXIT_CLOSED);
        assert_eq!(Outcome::TimedOut.exit_code(), EXIT_TIMED_OUT);
        assert_eq!(Outcome::Interrupted.exit_code(), 130);
        // distinct from show's codes
        for c in [EXIT_CLOSED, EXIT_TIMED_OUT] {
            assert!(
                ![
                    crate::show::EXIT_INVALID,
                    crate::show::EXIT_SHOW_FAILED,
                    crate::show::EXIT_NOT_INSTALLED
                ]
                .contains(&c)
            );
        }
    }
}
