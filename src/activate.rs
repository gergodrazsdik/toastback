// SPDX-License-Identifier: GPL-3.0-or-later
//! The `toastback://` protocol handler: check the click, run its callback, log the result.

use anyhow::{Result, bail};

use crate::spec::{BODY_ACTION, OnClick, Spec, is_token};

#[derive(Debug, PartialEq, Eq)]
pub struct Activation {
    pub id: String,
    pub action: String,
    pub token: String,
}

/// Parse `toastback://act?id=…&a=…&t=…`. Windows hands the handler `toastback://act/?…`, so both
/// forms are accepted. Every value must be a plain token (no percent-encoding is ever needed).
pub fn parse_uri(uri: &str) -> Result<Activation> {
    let Some(rest) = uri.strip_prefix("toastback://act") else {
        bail!("not a toastback activation URI");
    };
    let Some(query) = rest.strip_prefix("/?").or_else(|| rest.strip_prefix('?')) else {
        bail!("missing query");
    };
    let (mut id, mut action, mut token) = (None, None, None);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let slot = match k {
            "id" => &mut id,
            "a" => &mut action,
            "t" => &mut token,
            _ => bail!("unknown parameter {k:?}"),
        };
        if slot.is_some() {
            bail!("repeated parameter {k:?}");
        }
        if !is_token(v, 64) {
            bail!("bad value for {k:?}");
        }
        *slot = Some(v.to_string());
    }
    match (id, action, token) {
        (Some(id), Some(action), Some(token)) => Ok(Activation { id, action, token }),
        _ => bail!("needs id, a and t"),
    }
}

/// The `TOASTBACK_*` variables a callback gets.
pub fn callback_vars(spec: &crate::spec::Spec, id: &str, action: &str) -> Vec<(String, String)> {
    let data = spec
        .data
        .as_ref()
        .map_or_else(|| "null".to_string(), |d| d.to_string());
    vec![
        ("TOASTBACK_ID".into(), id.into()),
        ("TOASTBACK_ACTION".into(), action.into()),
        ("TOASTBACK_TITLE".into(), spec.title.clone()),
        ("TOASTBACK_DATA".into(), data),
    ]
}

/// How a callback ended, for the log and the feedback toast.
#[derive(Debug, Clone, Default)]
pub struct Ended {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    /// The callback couldn't be started at all.
    pub error: Option<String>,
}

impl Ended {
    pub fn ok(&self) -> bool {
        self.error.is_none() && !self.timed_out && self.exit_code == Some(0)
    }
}

/// The "done"/"failed" toast after a callback, if the spec asks for one (`notify_on_success`,
/// `notify_on_error`). It has no buttons and no body action, and never asks for more feedback.
pub fn feedback(spec: &Spec, action: &str, ended: &Ended) -> Option<Spec> {
    let label = if action == BODY_ACTION {
        "Click".to_string()
    } else {
        spec.button(action)
            .map_or_else(|| action.to_string(), |b| b.label.clone())
    };
    let secs = ended.duration_ms as f64 / 1000.0;
    let last_line = |s: &str| {
        s.lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(|l| l.chars().take(200).collect::<String>())
    };
    let (title, mut body) = if ended.ok() {
        if !spec.notify_on_success() {
            return None;
        }
        let mut body = format!("{} · {secs:.1}s", spec.title);
        if let Some(l) = last_line(&ended.stdout) {
            body += &format!("\n{l}");
        }
        (format!("{label}: done"), body)
    } else {
        if !spec.notify_on_error() {
            return None;
        }
        let reason = match (&ended.error, ended.timed_out, ended.exit_code) {
            (Some(e), _, _) => format!("could not start: {e}"),
            (None, true, _) => format!("timed out after {secs:.1}s"),
            (None, false, Some(c)) => format!("exit code {c} after {secs:.1}s"),
            (None, false, None) => "killed".to_string(),
        };
        (
            format!("{label}: failed"),
            format!("{} · {reason}", spec.title),
        )
    };
    if !ended.ok()
        && let Some(l) = last_line(&ended.stderr).or_else(|| last_line(&ended.stdout))
    {
        body += &format!("\n{l}");
    }
    Some(Spec {
        title,
        body: Some(body),
        attribution: Some("toastback · details: toastback log".into()),
        expires: Some("1d".into()),
        on_click: Some(OnClick::None),
        notify_on_error: Some(false),
        ..Default::default()
    })
}

#[cfg(windows)]
pub use imp::{dialog, handle};

#[cfg(windows)]
mod imp {
    use anyhow::Result;
    use serde_json::json;

    use super::{Ended, callback_vars, feedback, parse_uri};
    use crate::dialog as td;
    use crate::exec;
    use crate::show;
    use crate::spec::{BODY_ACTION, OnClick, Run};
    use crate::store::{Pending, Store, token_matches};
    use crate::toast;

    /// Handle one toast activation. Nobody sees this process's stderr, so every outcome is logged.
    pub fn handle(store: &Store, uri: &str) -> Result<()> {
        let act = match parse_uri(uri) {
            Ok(a) => a,
            Err(e) => {
                return store.log(
                    "rejected",
                    json!({ "uri": uri, "reason": format!("{e:#}") }),
                );
            }
        };
        let reject = |reason: &str| {
            store.log(
                "rejected",
                json!({ "id": act.id, "action": act.action, "reason": reason, "uri": uri }),
            )
        };
        let Some(pending) = store.load_pending(&act.id)? else {
            return reject("unknown or expired id");
        };
        if !token_matches(&pending.token, &act.token) {
            return reject("token mismatch");
        }
        if pending.done {
            return reject("already handled");
        }
        if act.action != BODY_ACTION && pending.spec.button(&act.action).is_none() {
            return reject("unknown button");
        }
        store.log("activated", json!({ "id": act.id, "action": act.action }))?;

        let action = if act.action == BODY_ACTION {
            match pending.spec.body_click() {
                OnClick::Default => Some(BODY_ACTION.to_string()),
                OnClick::Details => ask(store, &pending)?,
                OnClick::None => None,
            }
        } else {
            Some(act.action)
        };
        finish(store, pending, action.as_deref())
    }

    /// `__dialog <id>`: the detached process behind `mode: dialog`.
    pub fn dialog(store: &Store, id: &str) -> Result<()> {
        let Some(pending) = store.load_pending(id)? else {
            return store.log(
                "rejected",
                json!({ "id": id, "reason": "unknown or expired id" }),
            );
        };
        if pending.done {
            return store.log("rejected", json!({ "id": id, "reason": "already handled" }));
        }
        let action = ask(store, &pending)?;
        finish(store, pending, action.as_deref())
    }

    /// Show the dialog for a notification; returns the chosen button id.
    fn ask(store: &Store, pending: &Pending) -> Result<Option<String>> {
        let id = pending.spec.id.as_deref().unwrap_or_default();
        store.log("dialog_shown", json!({ "id": id }))?;
        let choice = td::show(&td::model(&pending.spec))?;
        store.log(
            "dialog_closed",
            json!({ "id": id, "action": choice.as_deref().unwrap_or("(closed)") }),
        )?;
        Ok(choice)
    }

    /// Run the callback for `action` (if any), mark the notification handled, clear its toast.
    fn finish(store: &Store, mut pending: Pending, action: Option<&str>) -> Result<()> {
        let id = pending.spec.id.clone().unwrap_or_default();
        // a dialog can stay open for long: was the notification removed (or handled) meanwhile?
        if !store.load_pending(&id)?.is_some_and(|p| !p.done) {
            return store.log(
                "rejected",
                json!({ "id": id, "action": action, "reason": "removed or already handled" }),
            );
        }
        if let Some(action) = action
            && let Some(run) = pending.spec.run_for(action)
        {
            run_callback(store, &pending, &id, action, run)?;
        }
        pending.done = true;
        store.save_pending(&pending)?;
        if let Err(e) = toast::remove(&id) {
            store.log(
                "warning",
                json!({ "id": id, "error": format!("remove toast: {e:#}") }),
            )?;
        }
        Ok(())
    }

    fn run_callback(
        store: &Store,
        pending: &Pending,
        id: &str,
        action: &str,
        run: &Run,
    ) -> Result<()> {
        let spec = &pending.spec;
        let vars = callback_vars(spec, id, action);
        let wslenv = std::env::var("WSLENV").ok();
        let result = exec::plan(run, &spec.context, &vars, wslenv.as_deref()).and_then(|plan| {
            store.log(
                "callback_started",
                json!({ "id": id, "action": action, "plan": format!("{plan:?}") }),
            )?;
            exec::execute(&plan, run.timeout()?)
        });
        let ended = match result {
            Ok(o) => {
                let ended = Ended {
                    exit_code: o.exit_code,
                    timed_out: o.timed_out,
                    duration_ms: o.duration.as_millis() as u64,
                    stdout: o.stdout,
                    stderr: o.stderr,
                    error: None,
                };
                store.log(
                    "callback_finished",
                    json!({
                        "id": id, "action": action,
                        "exit_code": ended.exit_code, "timed_out": ended.timed_out,
                        "duration_ms": ended.duration_ms,
                        "stdout": ended.stdout, "stderr": ended.stderr,
                    }),
                )?;
                ended
            }
            Err(e) => {
                let error = format!("{e:#}");
                store.log(
                    "callback_failed",
                    json!({ "id": id, "action": action, "error": error }),
                )?;
                Ended {
                    error: Some(error),
                    ..Default::default()
                }
            }
        };
        if let Some(note) = feedback(spec, action, &ended)
            && let Err((_, e)) = show::show_spec(store, note)
        {
            store.log(
                "warning",
                json!({ "id": id, "error": format!("feedback toast: {e:#}") }),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_forms() {
        let want = Activation {
            id: "m3x-0a1b".into(),
            action: "deploy".into(),
            token: "00ff".into(),
        };
        assert_eq!(
            parse_uri("toastback://act?id=m3x-0a1b&a=deploy&t=00ff").unwrap(),
            want
        );
        assert_eq!(
            parse_uri("toastback://act/?id=m3x-0a1b&a=deploy&t=00ff").unwrap(),
            want
        );
        assert_eq!(
            parse_uri("toastback://act/?a=deploy&t=00ff&id=m3x-0a1b").unwrap(),
            want
        );
    }

    #[test]
    fn rejects_junk() {
        for uri in [
            "",
            "http://act?id=a&a=b&t=c",
            "toastback://other?id=a&a=b&t=c",
            "toastback://act",
            "toastback://act?id=a&a=b",
            "toastback://act?id=a&a=b&t=c&x=1",
            "toastback://act?id=a&id=b&a=b&t=c",
            "toastback://act?id=a/../b&a=b&t=c",
            "toastback://act?id=a&a=b&t=c%20d",
            "toastback://act?id=&a=b&t=c",
        ] {
            assert!(parse_uri(uri).is_err(), "{uri}");
        }
    }

    #[test]
    fn feedback_toasts() {
        let spec = Spec::parse(
            r#"{"title": "Deploy?", "notify_on_success": true,
                "buttons": [{"id": "go", "label": "Deploy", "run": {"wsl": "x"}}]}"#,
        )
        .unwrap();
        let ok = Ended {
            exit_code: Some(0),
            duration_ms: 1234,
            stdout: "step 1\ndeployed v2\n\n".into(),
            ..Default::default()
        };
        let s = feedback(&spec, "go", &ok).unwrap();
        assert_eq!(s.title, "Deploy: done");
        assert_eq!(s.body.as_deref(), Some("Deploy? · 1.2s\ndeployed v2"));
        assert_eq!(s.body_click(), OnClick::None);
        assert!(!s.notify_on_error());
        s.validate().unwrap();

        let failed = Ended {
            exit_code: Some(3),
            duration_ms: 200,
            stderr: "oops".into(),
            ..Default::default()
        };
        let s = feedback(&spec, "go", &failed).unwrap();
        assert_eq!(s.title, "Deploy: failed");
        assert_eq!(
            s.body.as_deref(),
            Some("Deploy? · exit code 3 after 0.2s\noops")
        );

        let timeout = Ended {
            timed_out: true,
            duration_ms: 2000,
            ..Default::default()
        };
        assert!(
            feedback(&spec, BODY_ACTION, &timeout)
                .unwrap()
                .body
                .unwrap()
                .contains("timed out after 2.0s")
        );
        let not_started = Ended {
            error: Some("start wsl.exe: not found".into()),
            ..Default::default()
        };
        assert_eq!(
            feedback(&spec, "go", &not_started).unwrap().body.as_deref(),
            Some("Deploy? · could not start: start wsl.exe: not found")
        );

        // defaults: errors on, success off
        let quiet = Spec::parse(r#"{"title": "t"}"#).unwrap();
        assert!(feedback(&quiet, BODY_ACTION, &ok).is_none());
        assert!(feedback(&quiet, BODY_ACTION, &failed).is_some());
        let silent = Spec::parse(r#"{"title": "t", "notify_on_error": false}"#).unwrap();
        assert!(feedback(&silent, BODY_ACTION, &failed).is_none());
    }

    #[test]
    fn vars_carry_data_as_json() {
        let s = crate::spec::Spec::parse(r#"{"title": "T", "data": {"build": 1234}}"#).unwrap();
        let v = callback_vars(&s, "i", "go");
        assert!(v.contains(&("TOASTBACK_DATA".into(), r#"{"build":1234}"#.into())));
        assert!(v.contains(&("TOASTBACK_ACTION".into(), "go".into())));
    }
}
