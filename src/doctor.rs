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

#[cfg(windows)]
pub use imp::run;

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::UI::Notifications::NotificationSetting;

    use super::{Check, Status};
    use crate::install::{
        self, ENV_KEY, PROTOCOL_COMMAND_KEY, aumid_key, path_contains, protocol_command,
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
