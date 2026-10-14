// SPDX-License-Identifier: GPL-3.0-or-later
//! The `toastback://` protocol handler: check the click, run its callback, log the result.

use anyhow::{Result, bail};

use crate::spec::is_token;

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

#[cfg(windows)]
pub use imp::handle;

#[cfg(windows)]
mod imp {
    use anyhow::Result;
    use serde_json::json;

    use super::{callback_vars, parse_uri};
    use crate::exec;
    use crate::spec::BODY_ACTION;
    use crate::spec::OnClick;
    use crate::store::{Store, token_matches};
    use crate::toast;

    /// Handle one activation. Nobody sees this process's stderr, so every outcome goes to the log.
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
        let Some(mut pending) = store.load_pending(&act.id)? else {
            return reject("unknown or expired id");
        };
        if !token_matches(&pending.token, &act.token) {
            return reject("token mismatch");
        }
        if pending.done {
            return reject("already handled");
        }
        let spec = &pending.spec;
        if act.action != BODY_ACTION && spec.button(&act.action).is_none() {
            return reject("unknown button");
        }
        store.log("activated", json!({ "id": act.id, "action": act.action }))?;

        let run = if act.action == BODY_ACTION {
            match spec.body_click() {
                OnClick::Default => spec.run_for(BODY_ACTION),
                OnClick::Details => {
                    // details dialog: M2
                    store.log(
                        "skipped",
                        json!({ "id": act.id, "reason": "details dialog not implemented yet" }),
                    )?;
                    None
                }
                OnClick::None => None,
            }
        } else {
            spec.run_for(&act.action)
        };

        if let Some(run) = run {
            let vars = callback_vars(spec, &act.id, &act.action);
            let wslenv = std::env::var("WSLENV").ok();
            let result =
                exec::plan(run, &spec.context, &vars, wslenv.as_deref()).and_then(|plan| {
                    store.log(
                        "callback_started",
                        json!({ "id": act.id, "action": act.action, "plan": format!("{plan:?}") }),
                    )?;
                    exec::execute(&plan, run.timeout()?)
                });
            match result {
                Ok(o) => store.log(
                    "callback_finished",
                    json!({
                        "id": act.id, "action": act.action,
                        "exit_code": o.exit_code, "timed_out": o.timed_out,
                        "duration_ms": o.duration.as_millis() as u64,
                        "stdout": o.stdout, "stderr": o.stderr,
                    }),
                )?,
                Err(e) => store.log(
                    "callback_failed",
                    json!({ "id": act.id, "action": act.action, "error": format!("{e:#}") }),
                )?,
            }
            // notify_on_error error toast: M3
        }

        pending.done = true;
        store.save_pending(&pending)?;
        if let Err(e) = toast::remove(&act.id) {
            store.log(
                "warning",
                json!({ "id": act.id, "error": format!("remove toast: {e:#}") }),
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
    fn vars_carry_data_as_json() {
        let s = crate::spec::Spec::parse(r#"{"title": "T", "data": {"build": 1234}}"#).unwrap();
        let v = callback_vars(&s, "i", "go");
        assert!(v.contains(&("TOASTBACK_DATA".into(), r#"{"build":1234}"#.into())));
        assert!(v.contains(&("TOASTBACK_ACTION".into(), "go".into())));
    }
}
