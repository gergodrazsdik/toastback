// SPDX-License-Identifier: GPL-3.0-or-later
//! `doctor`: check that the installation, registration, notifications and WSL shim are in order.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

impl Check {
    pub fn new(name: impl Into<String>, status: Status, detail: impl Into<String>) -> Check {
        Check {
            name: name.into(),
            status,
            detail: detail.into(),
        }
    }
}

/// One line per check: `ok    name         detail`.
pub fn render(checks: &[Check]) -> String {
    let width = checks.iter().map(|c| c.name.len()).max().unwrap_or(0);
    checks
        .iter()
        .map(|c| {
            let status = match c.status {
                Status::Ok => "ok",
                Status::Warn => "WARN",
                Status::Fail => "FAIL",
            };
            format!("{status:<5} {:<width$}  {}\n", c.name, c.detail)
        })
        .collect()
}

/// Exit code: 1 if anything failed (warnings don't count).
pub fn exit_code(checks: &[Check]) -> u8 {
    u8::from(checks.iter().any(|c| c.status == Status::Fail))
}

/// The notify-send wrapper in the caller's distro: `declined` after `install --no-notify-send`.
pub fn notify_send_check(
    s: &crate::install::NotifySendStatus,
    declined: bool,
    version: &str,
) -> (Status, String) {
    let theirs = s.content.is_some() && !s.is_ours();
    if declined && (s.content.is_none() || theirs) {
        return (Status::Ok, "off (install --no-notify-send)".into());
    }
    if s.content.is_none() {
        return (
            Status::Warn,
            "missing: run `toastback.exe install` (or `install --no-notify-send` to keep it off)"
                .into(),
        );
    }
    if theirs {
        return (
            Status::Warn,
            format!("{} isn't toastback's, so install left it alone", s.path),
        );
    }
    if s.content.as_deref() != Some(&crate::install::render_notify_send(version)) {
        return (Status::Warn, "outdated: run `toastback.exe install`".into());
    }
    if s.resolved != s.path {
        let why = if s.resolved.is_empty() {
            "~/.local/bin is not on PATH".to_string()
        } else {
            format!("{} comes first on PATH", s.resolved)
        };
        return (Status::Warn, format!("not used: {why}"));
    }
    (Status::Ok, format!("{} shows toasts", s.path))
}

/// The agent skill in the caller's distro, after `install --skill` (ADR 0021).
pub fn skill_check(content: Option<&str>, version: &str) -> (Status, String) {
    match content {
        None => (
            Status::Warn,
            "missing: run `toastback.exe install` (or `install --no-skill` to turn it off)".into(),
        ),
        Some(c) if !crate::skill::is_ours(c) => (
            Status::Warn,
            format!(
                "~/{} isn't toastback's, so install left it alone",
                crate::skill::SKILL_PATH
            ),
        ),
        Some(c) if c != crate::skill::render(version) => {
            (Status::Warn, "outdated: run `toastback.exe install`".into())
        }
        Some(_) => (
            Status::Ok,
            format!("~/{} is current", crate::skill::SKILL_PATH),
        ),
    }
}

#[cfg(windows)]
pub use imp::run;

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::UI::Notifications::NotificationSetting;

    use super::{Check, Status};
    use crate::install::{
        self, APP_KEY, ENV_KEY, PROTOCOL_COMMAND_KEY, aumid_key, path_contains, protocol_command,
        render_shim,
    };
    use crate::registry;
    use crate::spec::Context;
    use crate::store::{ShimTarget, Store};
    use crate::toast;

    pub fn run(store: &Store, ctx: &Context, version: &str) -> Vec<Check> {
        use Status::{Fail as Bad, Ok as Good, Warn};
        let mut checks = Vec::new();
        let exe = store.installed_exe();
        let exe_str = exe.to_string_lossy().into_owned();

        let record = match store.install_record() {
            Ok(Some(r)) => {
                checks.push(Check::new(
                    "installed",
                    Good,
                    format!("version {}, {}", r.version, r.installed_at),
                ));
                if r.version != version {
                    checks.push(Check::new(
                        "version",
                        Warn,
                        format!(
                            "installed {}, running {version}: run `install` again",
                            r.version
                        ),
                    ));
                }
                Some(r)
            }
            Ok(None) => {
                checks.push(Check::new(
                    "installed",
                    Bad,
                    "not installed: run `toastback.exe install`",
                ));
                None
            }
            Err(e) => {
                checks.push(Check::new("installed", Bad, format!("{e:#}")));
                None
            }
        };
        checks.push(if exe.exists() {
            Check::new("exe", Good, &exe_str)
        } else {
            Check::new("exe", Bad, format!("missing: {exe_str}"))
        });

        checks.push(match registry::get_string(&aumid_key(), "IconUri") {
            Ok(Some((icon, _))) if Path::new(&icon).exists() => {
                Check::new("app id", Good, "registered")
            }
            Ok(Some((icon, _))) => Check::new("app id", Warn, format!("icon missing: {icon}")),
            Ok(None) => Check::new("app id", Bad, "not registered"),
            Err(e) => Check::new("app id", Bad, format!("{e:#}")),
        });
        let want = protocol_command(&exe_str);
        checks.push(match registry::get_string(PROTOCOL_COMMAND_KEY, "") {
            Ok(Some((cmd, _))) if cmd == want => {
                Check::new("toastback://", Good, "handled by the installed exe")
            }
            Ok(Some((cmd, _))) => Check::new("toastback://", Bad, format!("points at {cmd}")),
            Ok(None) => Check::new("toastback://", Bad, "not registered"),
            Err(e) => Check::new("toastback://", Bad, format!("{e:#}")),
        });
        checks.push(match registry::get_string(APP_KEY, "DisplayVersion") {
            Ok(Some((v, _))) if record.as_ref().is_some_and(|r| r.version == v) => {
                Check::new("app entry", Good, format!("version {v}"))
            }
            Ok(Some((v, _))) => Check::new(
                "app entry",
                Warn,
                format!("says version {v}: run `install` again"),
            ),
            Ok(None) => Check::new(
                "app entry",
                Warn,
                "not in Apps & Features: run `install` again",
            ),
            Err(e) => Check::new("app entry", Warn, format!("{e:#}")),
        });

        checks.push(match toast::setting() {
            Ok(Some(s)) if s == NotificationSetting::Enabled => {
                Check::new("notifications", Good, "enabled")
            }
            Ok(Some(s)) => {
                let why = if s == NotificationSetting::DisabledForApplication {
                    "turned off for toastback"
                } else if s == NotificationSetting::DisabledForUser {
                    "turned off for all apps"
                } else if s == NotificationSetting::DisabledByGroupPolicy {
                    "turned off by group policy"
                } else {
                    "turned off"
                };
                Check::new(
                    "notifications",
                    Bad,
                    format!("{why}: Settings > System > Notifications"),
                )
            }
            Ok(None) => Check::new(
                "notifications",
                Warn,
                "unknown until the first toast is shown: try `toastback -t test`",
            ),
            Err(e) => Check::new(
                "notifications",
                Warn,
                format!("unavailable in this session: {e:#}"),
            ),
        });

        if record.as_ref().is_some_and(|r| r.path_added) {
            let bin = store.bin_dir().to_string_lossy().into_owned();
            let on_path = registry::get_string(ENV_KEY, "Path")
                .ok()
                .flatten()
                .is_some_and(|(p, _)| path_contains(&p, &bin));
            checks.push(if on_path {
                Check::new("user PATH", Good, "contains the install dir")
            } else {
                Check::new("user PATH", Warn, format!("{bin} was removed from it"))
            });
        }

        checks.push(match install::list_distros() {
            Ok(d) if d.is_empty() => Check::new(
                "wsl.exe",
                Warn,
                "no WSL distros installed (only needed for wsl callbacks)",
            ),
            Ok(d) => Check::new("wsl.exe", Good, format!("distros: {}", d.join(", "))),
            Err(e) => Check::new(
                "wsl.exe",
                Warn,
                format!("{e:#} (only needed for wsl callbacks)"),
            ),
        });
        if let Some(distro) = &ctx.distro {
            let target = ShimTarget {
                distro: distro.clone(),
                user: ctx.user.clone(),
            };
            let name = format!("shim ({distro})");
            checks.push(match install::read_shim(&target) {
                Ok(Some(s)) if s == render_shim(version, &exe_str) => {
                    Check::new(name, Good, "~/.local/bin/toastback is current")
                }
                Ok(Some(_)) => Check::new(name, Warn, "outdated: run `toastback.exe install`"),
                Ok(None) => Check::new(name, Bad, "missing: run `toastback.exe install`"),
                Err(e) => Check::new(name, Warn, format!("{e:#}")),
            });
            let name = format!("notify-send ({distro})");
            let declined = record.as_ref().is_some_and(|r| r.no_notify_send);
            checks.push(match install::read_notify_send(&target) {
                Ok(s) => {
                    let (status, detail) = super::notify_send_check(&s, declined, version);
                    Check::new(name, status, detail)
                }
                Err(e) => Check::new(name, Warn, format!("{e:#}")),
            });
            if record.as_ref().is_some_and(|r| r.skill) {
                let name = format!("agent skill ({distro})");
                checks.push(match install::read_skill(&target) {
                    Ok(content) => {
                        let (status, detail) = super::skill_check(content.as_deref(), version);
                        Check::new(name, status, detail)
                    }
                    Err(e) => Check::new(name, Warn, format!("{e:#}")),
                });
            }
        }

        let probe = store.root().join("doctor.tmp");
        let writable = std::fs::create_dir_all(store.root())
            .and_then(|()| std::fs::write(&probe, b"x"))
            .and_then(|()| std::fs::remove_file(&probe));
        checks.push(match writable {
            Ok(()) => Check::new("state dir", Good, store.root().to_string_lossy()),
            Err(e) => Check::new("state dir", Bad, format!("{}: {e}", store.root().display())),
        });
        checks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_send_states() {
        use crate::install::{NotifySendStatus, render_notify_send};
        let ours = render_notify_send("1.5.0");
        let status = |content: Option<&str>, resolved: &str| NotifySendStatus {
            path: "/home/g/.local/bin/notify-send".into(),
            resolved: resolved.into(),
            content: content.map(str::to_string),
        };
        let path = "/home/g/.local/bin/notify-send";
        let check = |s: &NotifySendStatus, declined| notify_send_check(s, declined, "1.5.0").0;
        assert_eq!(check(&status(Some(&ours), path), false), Status::Ok);
        assert_eq!(check(&status(Some(&ours), path), true), Status::Ok);
        assert_eq!(
            check(&status(None, "/usr/bin/notify-send"), false),
            Status::Warn
        );
        assert_eq!(check(&status(None, ""), true), Status::Ok);
        assert_eq!(
            check(&status(Some("#!/bin/sh\nmine\n"), path), false),
            Status::Warn
        );
        assert_eq!(
            check(&status(Some("#!/bin/sh\nmine\n"), path), true),
            Status::Ok
        );
        let old = render_notify_send("1.4.0");
        assert!(
            notify_send_check(&status(Some(&old), path), false, "1.5.0")
                .1
                .contains("outdated")
        );
        let (s, detail) = notify_send_check(
            &status(Some(&ours), "/usr/local/bin/notify-send"),
            false,
            "1.5.0",
        );
        assert_eq!(s, Status::Warn);
        assert!(
            detail.contains("/usr/local/bin/notify-send comes first"),
            "{detail}"
        );
        let detail = notify_send_check(&status(Some(&ours), ""), false, "1.5.0").1;
        assert!(detail.contains("not on PATH"), "{detail}");
    }

    #[test]
    fn skill_states() {
        let ours = crate::skill::render("1.6.0");
        assert_eq!(skill_check(Some(&ours), "1.6.0").0, Status::Ok);
        let (s, detail) = skill_check(Some(&crate::skill::render("1.5.0")), "1.6.0");
        assert_eq!((s, detail.starts_with("outdated")), (Status::Warn, true));
        let (s, detail) = skill_check(Some("---\nname: mine\n---\n"), "1.6.0");
        assert_eq!(
            (s, detail.contains("isn't toastback's")),
            (Status::Warn, true)
        );
        assert!(skill_check(None, "1.6.0").1.starts_with("missing"));
    }

    #[test]
    fn renders_and_exit_code() {
        let checks = vec![
            Check::new("installed", Status::Ok, "version 0.1.0"),
            Check::new("notifications", Status::Warn, "unknown"),
        ];
        assert_eq!(
            render(&checks),
            "ok    installed      version 0.1.0\nWARN  notifications  unknown\n"
        );
        assert_eq!(exit_code(&checks), 0);
        let mut failing = checks.clone();
        failing.push(Check::new("exe", Status::Fail, "missing"));
        assert_eq!(exit_code(&failing), 1);
        assert_eq!(
            serde_json::to_value(&failing[2]).unwrap(),
            serde_json::json!({"name": "exe", "status": "fail", "detail": "missing"})
        );
    }
}
