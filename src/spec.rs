// SPDX-License-Identifier: GPL-3.0-or-later
//! The notification spec (PLAN.md §4): serde types, defaults, validation, merging quick flags.

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Serialize};

pub const MAX_BUTTONS: usize = 5;
/// Action id for a click on the toast body.
pub const BODY_ACTION: &str = "__body";
pub const DEFAULT_EXPIRES: &str = "7d";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Spec {
    pub id: Option<String>,
    pub mode: Mode,
    pub title: String,
    pub body: Option<String>,
    pub details: Option<String>,
    pub icon: Option<String>,
    pub image: Option<String>,
    pub attribution: Option<String>,
    pub scenario: Scenario,
    pub sound: Sound,
    pub expires: Option<String>,
    pub on_click: Option<OnClick>,
    pub buttons: Vec<Button>,
    pub default: Option<Run>,
    pub data: Option<serde_json::Value>,
    pub notify_on_error: Option<bool>,
    pub notify_on_success: Option<bool>,
    pub context: Context,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Toast,
    Dialog,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Scenario {
    #[default]
    Default,
    Reminder,
    Alarm,
    Urgent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sound {
    #[default]
    Default,
    Silent,
    Alarm,
    Reminder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnClick {
    Details,
    Default,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    Success,
    Critical,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Button {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub style: Option<Style>,
    #[serde(default)]
    pub run: Option<Run>,
}

/// Where a WSL callback runs: filled by the shim (`--distro/--user/--cwd`) or set in the spec.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Context {
    pub distro: Option<String>,
    pub user: Option<String>,
    pub cwd: Option<String>,
}

impl Context {
    /// Fill the fields the spec left empty. A context written in the spec wins over the shim's.
    pub fn fill_from(&mut self, other: &Context) {
        if self.distro.is_none() {
            self.distro.clone_from(&other.distro);
        }
        if self.user.is_none() {
            self.user.clone_from(&other.user);
        }
        if self.cwd.is_none() {
            self.cwd.clone_from(&other.cwd);
        }
    }
}

/// A callback: exactly one primary key (`wsl`, `cmd`, `pwsh`, `exe`, `open`) plus optional modifiers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Run {
    pub wsl: Option<WslCommand>,
    pub cmd: Option<String>,
    pub pwsh: Option<String>,
    pub exe: Option<Vec<String>>,
    pub open: Option<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub distro: Option<String>,
    pub user: Option<String>,
    pub timeout: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WslCommand {
    Shell(String),
    Argv(Vec<String>),
}

/// The primary key of a [`Run`], borrowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunKind<'a> {
    WslShell(&'a str),
    WslArgv(&'a [String]),
    Cmd(&'a str),
    Pwsh(&'a str),
    Exe(&'a [String]),
    Open(&'a str),
}

impl Run {
    pub fn kind(&self) -> Result<RunKind<'_>> {
        let mut kinds = Vec::new();
        match &self.wsl {
            Some(WslCommand::Shell(s)) => kinds.push(RunKind::WslShell(s)),
            Some(WslCommand::Argv(a)) => kinds.push(RunKind::WslArgv(a)),
            None => {}
        }
        if let Some(s) = &self.cmd {
            kinds.push(RunKind::Cmd(s));
        }
        if let Some(s) = &self.pwsh {
            kinds.push(RunKind::Pwsh(s));
        }
        if let Some(a) = &self.exe {
            kinds.push(RunKind::Exe(a));
        }
        if let Some(s) = &self.open {
            kinds.push(RunKind::Open(s));
        }
        match kinds.as_slice() {
            [one] => Ok(*one),
            [] => bail!("needs one of wsl, cmd, pwsh, exe, open"),
            _ => bail!("has more than one of wsl, cmd, pwsh, exe, open"),
        }
    }

    pub fn timeout(&self) -> Result<Option<Duration>> {
        self.timeout.as_deref().map(parse_duration).transpose()
    }

    fn validate(&self) -> Result<()> {
        match self.kind()? {
            RunKind::WslShell(s) | RunKind::Cmd(s) | RunKind::Pwsh(s) | RunKind::Open(s) => {
                ensure!(!s.trim().is_empty(), "command is empty");
            }
            RunKind::WslArgv(a) | RunKind::Exe(a) => {
                ensure!(
                    a.first().is_some_and(|p| !p.is_empty()),
                    "argv needs a program"
                );
            }
        }
        if let RunKind::Open(_) = self.kind()? {
            ensure!(
                self.env.is_empty() && self.timeout.is_none(),
                "open takes no env or timeout"
            );
        }
        for key in self.env.keys() {
            ensure!(is_env_name(key), "env: invalid variable name {key:?}");
        }
        self.timeout().context("timeout")?;
        Ok(())
    }
}

/// Quick CLI flags; each one set overrides the spec field.
#[derive(Debug, Clone, Default)]
pub struct QuickFlags {
    pub id: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub details: Option<String>,
    pub icon: Option<String>,
    pub mode: Option<Mode>,
    pub scenario: Option<Scenario>,
}

impl QuickFlags {
    pub fn is_empty(&self) -> bool {
        self.id.is_none()
            && self.title.is_none()
            && self.body.is_none()
            && self.details.is_none()
            && self.icon.is_none()
            && self.mode.is_none()
            && self.scenario.is_none()
    }
}

impl Spec {
    pub fn parse(json: &str) -> Result<Spec> {
        serde_json::from_str(json).context("invalid spec JSON")
    }

    pub fn apply(&mut self, f: &QuickFlags) {
        if f.id.is_some() {
            self.id.clone_from(&f.id);
        }
        if let Some(t) = &f.title {
            self.title.clone_from(t);
        }
        if f.body.is_some() {
            self.body.clone_from(&f.body);
        }
        if f.details.is_some() {
            self.details.clone_from(&f.details);
        }
        if f.icon.is_some() {
            self.icon.clone_from(&f.icon);
        }
        if let Some(m) = f.mode {
            self.mode = m;
        }
        if let Some(s) = f.scenario {
            self.scenario = s;
        }
    }

    pub fn expires(&self) -> Result<Duration> {
        parse_duration(self.expires.as_deref().unwrap_or(DEFAULT_EXPIRES))
    }

    /// Show an error toast when a callback fails (default: on).
    pub fn notify_on_error(&self) -> bool {
        self.notify_on_error.unwrap_or(true)
    }

    /// Show a small "done" toast when a callback succeeds (default: off).
    pub fn notify_on_success(&self) -> bool {
        self.notify_on_success.unwrap_or(false)
    }

    pub fn button(&self, id: &str) -> Option<&Button> {
        self.buttons.iter().find(|b| b.id == id)
    }

    /// What a body click does, after defaults: `None` when there's nothing to run or show.
    pub fn body_click(&self) -> OnClick {
        let wanted = self.on_click.unwrap_or(if self.details.is_some() {
            OnClick::Details
        } else {
            OnClick::Default
        });
        match wanted {
            OnClick::Default if self.default.is_none() => OnClick::None,
            other => other,
        }
    }

    /// The callback for a button: its own `run`, else the notification-level `default`.
    pub fn run_for(&self, action: &str) -> Option<&Run> {
        if action == BODY_ACTION {
            return self.default.as_ref();
        }
        self.button(action)
            .and_then(|b| b.run.as_ref())
            .or(self.default.as_ref())
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(id) = &self.id {
            ensure!(
                is_token(id, 64),
                "id {id:?}: use 1-64 characters from A-Z a-z 0-9 . _ -"
            );
        }
        ensure!(!self.title.trim().is_empty(), "title is required");
        ensure!(
            self.buttons.len() <= MAX_BUTTONS,
            "at most {MAX_BUTTONS} buttons"
        );
        if self.scenario != Scenario::Default {
            ensure!(
                !self.buttons.is_empty(),
                "scenario {} needs at least one button",
                serde_json::to_value(self.scenario).unwrap_or_default()
            );
        }
        if self.on_click == Some(OnClick::Details) {
            ensure!(self.details.is_some(), "on_click \"details\" needs details");
        }
        self.expires().context("expires")?;
        let mut seen = HashSet::new();
        for b in &self.buttons {
            ensure!(
                is_token(&b.id, 32) && !b.id.starts_with("__"),
                "button id {:?}: use 1-32 characters from A-Z a-z 0-9 . _ - (not starting with __)",
                b.id
            );
            ensure!(seen.insert(&b.id), "duplicate button id {:?}", b.id);
            ensure!(
                !b.label.trim().is_empty(),
                "button {:?}: label is required",
                b.id
            );
            if let Some(r) = &b.run {
                r.validate()
                    .with_context(|| format!("button {:?}: run", b.id))?;
            }
        }
        if let Some(r) = &self.default {
            r.validate().context("default")?;
        }
        Ok(())
    }
}

/// `[A-Za-z0-9._-]{1,max}`: safe in file names, URIs and toast tags.
pub fn is_token(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

fn is_env_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Durations like `30s`, `5m`, `12h`, `7d`, or combined: `1h30m`.
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    ensure!(!s.is_empty(), "empty duration");
    let mut total = 0u64;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let unit = match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => bail!("{s:?}: unknown unit {c:?} (use s, m, h, d)"),
        };
        ensure!(!num.is_empty(), "{s:?}: missing number before {c:?}");
        let n: u64 = num.parse().with_context(|| format!("{s:?}: bad number"))?;
        total = total.saturating_add(n.saturating_mul(unit));
        num.clear();
    }
    ensure!(num.is_empty(), "{s:?}: missing unit (use s, m, h, d)");
    ensure!(total > 0, "{s:?}: must be longer than 0");
    Ok(Duration::from_secs(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(json: &str) -> Spec {
        Spec::parse(json).unwrap()
    }

    #[test]
    fn plan_example_parses_and_validates() {
        let s = spec(
            r#"{
            "id": "deploy-1234", "mode": "toast", "title": "Deploy to production?",
            "body": "Build 1234 passed all checks.", "details": "Long text",
            "icon": "/home/g/icons/rocket.png", "image": null, "attribution": "CI",
            "scenario": "default", "sound": "default", "expires": "7d", "on_click": "details",
            "buttons": [
              { "id": "deploy", "label": "Deploy", "style": "success",
                "run": { "wsl": "./scripts/deploy.sh --env prod" } },
              { "id": "logs", "label": "Open logs", "run": { "open": "https://ci.example.com/build/1234" } },
              { "id": "later", "label": "Not now" }
            ],
            "default": { "wsl": "echo \"$TOASTBACK_ACTION\" >> ~/toastback-clicks.log" },
            "data": { "build": 1234 }, "notify_on_error": true,
            "context": { "distro": "Ubuntu-24.04", "user": "g", "cwd": "/home/g/app" }
        }"#,
        );
        s.validate().unwrap();
        assert_eq!(s.body_click(), OnClick::Details);
        assert_eq!(
            s.run_for("deploy").unwrap().kind().unwrap(),
            RunKind::WslShell("./scripts/deploy.sh --env prod")
        );
        // "later" has no run: falls back to the default handler
        assert!(matches!(
            s.run_for("later").unwrap().kind().unwrap(),
            RunKind::WslShell(_)
        ));
    }

    #[test]
    fn minimal_spec() {
        let s = spec(r#"{"title": "hi"}"#);
        s.validate().unwrap();
        assert_eq!(s.mode, Mode::Toast);
        assert_eq!(s.expires().unwrap(), Duration::from_secs(7 * 86400));
        assert_eq!(s.body_click(), OnClick::None);
        assert!(s.notify_on_error());
        assert!(!s.notify_on_success());
    }

    #[test]
    fn rejects_bad_specs() {
        let bad = [
            r#"{"body": "no title"}"#,
            r#"{"title": "x", "id": "has space"}"#,
            r#"{"title": "x", "scenario": "alarm"}"#,
            r#"{"title": "x", "on_click": "details"}"#,
            r#"{"title": "x", "expires": "7 weeks"}"#,
            r#"{"title": "x", "buttons": [{"id": "a", "label": "A"}, {"id": "a", "label": "B"}]}"#,
            r#"{"title": "x", "buttons": [{"id": "__body", "label": "A"}]}"#,
            r#"{"title": "x", "buttons": [{"id": "a", "label": " "}]}"#,
            r#"{"title": "x", "buttons": [{"id": "a", "label": "A", "run": {}}]}"#,
            r#"{"title": "x", "buttons": [{"id": "a", "label": "A", "run": {"cmd": "x", "pwsh": "y"}}]}"#,
            r#"{"title": "x", "default": {"exe": []}}"#,
            r#"{"title": "x", "default": {"wsl": "x", "env": {"1BAD": "v"}}}"#,
            r#"{"title": "x", "default": {"wsl": "x", "timeout": "soon"}}"#,
            r#"{"title": "x", "default": {"open": "https://x", "timeout": "5m"}}"#,
        ];
        for json in bad {
            assert!(spec(json).validate().is_err(), "should reject {json}");
        }
        let six: Vec<String> = (0..6)
            .map(|i| format!(r#"{{"id": "b{i}", "label": "B"}}"#))
            .collect();
        let json = format!(r#"{{"title": "x", "buttons": [{}]}}"#, six.join(","));
        assert!(spec(&json).validate().is_err());
    }

    #[test]
    fn unknown_fields_are_errors() {
        assert!(Spec::parse(r#"{"title": "x", "titel": "y"}"#).is_err());
        assert!(Spec::parse(r#"{"title": "x", "default": {"shell": "y"}}"#).is_err());
        assert!(Spec::parse(r#"{"title": "x", "mode": "popup"}"#).is_err());
    }

    #[test]
    fn wsl_argv_form() {
        let s = spec(r#"{"title": "x", "default": {"wsl": ["ls", "-la"]}}"#);
        s.validate().unwrap();
        assert!(matches!(
            s.default.unwrap().kind().unwrap(),
            RunKind::WslArgv(a) if a == ["ls", "-la"]
        ));
    }

    #[test]
    fn body_click_defaults() {
        let with_default = spec(r#"{"title": "x", "default": {"cmd": "echo"}}"#);
        assert_eq!(with_default.body_click(), OnClick::Default);
        let none = spec(r#"{"title": "x", "on_click": "none", "default": {"cmd": "echo"}}"#);
        assert_eq!(none.body_click(), OnClick::None);
    }

    #[test]
    fn flags_override_spec() {
        let mut s = spec(r#"{"title": "from spec", "body": "keep"}"#);
        s.apply(&QuickFlags {
            title: Some("from flag".into()),
            mode: Some(Mode::Dialog),
            ..Default::default()
        });
        assert_eq!(s.title, "from flag");
        assert_eq!(s.body.as_deref(), Some("keep"));
        assert_eq!(s.mode, Mode::Dialog);
    }

    #[test]
    fn spec_context_wins_over_shim() {
        let mut c = Context {
            distro: Some("Spec".into()),
            ..Default::default()
        };
        c.fill_from(&Context {
            distro: Some("Shim".into()),
            user: Some("u".into()),
            cwd: None,
        });
        assert_eq!(c.distro.as_deref(), Some("Spec"));
        assert_eq!(c.user.as_deref(), Some("u"));
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_duration("7d").unwrap(), Duration::from_secs(604800));
        for bad in ["", "5", "m", "0s", "5w", "-5m"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }
}
