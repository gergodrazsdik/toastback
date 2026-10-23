// SPDX-License-Identifier: GPL-3.0-or-later
//! Command-line interface: arguments, `show` as the default command, dispatch, `log` output.

use anyhow::Result;
#[cfg(not(windows))]
use anyhow::bail;
use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use crate::VERSION;
#[cfg(windows)]
use crate::spec::Spec;
use crate::spec::{Context, Mode, QuickFlags, Scenario};
#[cfg(windows)]
use crate::{activate, doctor, examples, install, show, toast};

/// Async Windows toasts with callbacks, for WSL and scripts.
///
/// Shows a notification described by a JSON spec (FILE, or stdin) and/or quick flags, prints its
/// id and exits. Clicking a button later runs that button's callback.
/// Shown after the help of the top-level command and `show`.
const HELP_EXAMPLES: &str = "\
Examples:
  toastback -t \"Hello\" -m \"World\"                  a toast
  toastback -t \"Build failed\" -i error             with a built-in icon
  toastback --mode dialog -t \"Hello\" -m \"World\"    a dialog instead
  toastback examples                               ready-made specs with buttons and callbacks";

#[derive(Parser)]
#[command(version = VERSION, after_help = HELP_EXAMPLES)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
    #[command(flatten)]
    ctx: CtxArgs,
}

impl Cli {
    /// Parse, with `show` as the default command: `toastback [ctx flags] -t hi` means
    /// `toastback [ctx flags] show -t hi`.
    pub fn parse_with_default<I: Into<std::ffi::OsString>>(
        args: impl IntoIterator<Item = I>,
    ) -> Result<Cli, clap::Error> {
        use clap::CommandFactory;
        let mut args: Vec<std::ffi::OsString> = args.into_iter().map(Into::into).collect();
        let mut i = 1;
        while let Some(a) = args.get(i).and_then(|a| a.to_str()) {
            match a {
                "--distro" | "--user" | "--cwd" => i += 2,
                _ if ["--distro=", "--user=", "--cwd="]
                    .iter()
                    .any(|p| a.starts_with(p)) =>
                {
                    i += 1
                }
                _ => break,
            }
        }
        let first = args.get(i).and_then(|a| a.to_str());
        let is_command = first.is_some_and(|f| {
            matches!(f, "-h" | "--help" | "-V" | "--version" | "help")
                || Cli::command().get_subcommands().any(|c| c.get_name() == f)
        });
        if !is_command {
            args.insert(i.min(args.len()), "show".into());
        }
        attach_dash_values(&mut args);
        Cli::try_parse_from(args)
    }

    /// Is this the `show` command (also when it was the default)?
    pub fn is_show(&self) -> bool {
        matches!(self.command, Command::Show(_))
    }
}

/// Text for -t, -m and -d often comes from a script's output, which may start with `-` (`-- Build
/// files have been written`): pass such a value as `--body=<value>`, so clap takes it as the value.
/// A value that is one of our own flags stays a flag (`-t -m x` is still "-t needs a value").
fn attach_dash_values(args: &mut Vec<std::ffi::OsString>) {
    use clap::CommandFactory;
    const TEXT_FLAGS: [&str; 6] = ["-t", "--title", "-m", "--body", "-d", "--details"];
    let dash_value = |pair: &[std::ffi::OsString]| {
        pair[0].to_str().is_some_and(|f| TEXT_FLAGS.contains(&f))
            && pair[1].to_str().is_some_and(|v| v.starts_with('-'))
    };
    // the usual case (a click, a dialog, plain text): no need for the list of our flags
    if !args.windows(2).any(dash_value) {
        return;
    }
    let cmd = Cli::command();
    let mut flags: Vec<String> = vec!["--".into(), "-".into()];
    for c in std::iter::once(&cmd).chain(cmd.get_subcommands()) {
        for a in c.get_arguments() {
            flags.extend(a.get_short().map(|s| format!("-{s}")));
            flags.extend(a.get_long().map(|l| format!("--{l}")));
        }
    }
    flags.extend(["-h", "--help", "-V", "--version"].map(String::from));
    let is_flag = |v: &str| {
        flags.iter().any(|f| f == v)
            || v.split_once('=')
                .is_some_and(|(f, _)| f.starts_with("--") && flags.iter().any(|g| g == f))
    };
    let mut i = 0;
    while i + 1 < args.len() {
        let long = match args[i].to_str() {
            Some("-t" | "--title") => "--title",
            Some("-m" | "--body") => "--body",
            Some("-d" | "--details") => "--details",
            _ => {
                i += 1;
                continue;
            }
        };
        match args[i + 1].to_str() {
            Some(v) if v.starts_with('-') && !is_flag(v) => {
                args[i] = format!("{long}={v}").into();
                args.remove(i + 1);
            }
            _ => i += 2,
        }
    }
}

/// Where WSL callbacks run. The WSL shim always passes these.
#[derive(Args, Clone)]
struct CtxArgs {
    /// WSL distro of the caller
    #[arg(long, global = true)]
    distro: Option<String>,
    /// Linux user of the caller
    #[arg(long, global = true)]
    user: Option<String>,
    /// Linux working directory of the caller
    #[arg(long, global = true)]
    cwd: Option<String>,
}

impl CtxArgs {
    fn context(&self) -> Context {
        let nonempty = |s: &Option<String>| s.clone().filter(|s| !s.is_empty());
        Context {
            distro: nonempty(&self.distro),
            user: nonempty(&self.user),
            cwd: nonempty(&self.cwd),
        }
    }
}

#[derive(Args, Clone, Default)]
pub struct ShowArgs {
    /// JSON spec file, or - for stdin (default: stdin, unless -t/-m are given)
    pub file: Option<String>,
    /// Title (overrides the spec)
    #[arg(short = 't', long)]
    pub title: Option<String>,
    /// Body text
    #[arg(short = 'm', long = "body")]
    pub body: Option<String>,
    /// Details, shown in a dialog when the toast body is clicked
    #[arg(short = 'd', long)]
    pub details: Option<String>,
    /// Icon: info, warning, error, question, or a PNG/JPG (Windows or WSL path)
    #[arg(short = 'i', long)]
    pub icon: Option<String>,
    #[arg(long, value_enum)]
    pub mode: Option<Mode>,
    #[arg(long, value_enum)]
    pub scenario: Option<Scenario>,
    /// Notification id (default: generated)
    #[arg(long)]
    pub id: Option<String>,
}

impl ShowArgs {
    pub fn flags(&self) -> QuickFlags {
        QuickFlags {
            id: self.id.clone(),
            title: self.title.clone(),
            body: self.body.clone(),
            details: self.details.clone(),
            icon: self.icon.clone(),
            mode: self.mode,
            scenario: self.scenario,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Show a notification (the default command)
    #[command(after_help = HELP_EXAMPLES)]
    Show(ShowArgs),
    /// Install for the current user: copy the exes, register them, write the WSL shim
    Install {
        /// Distro(s) to write the shim to: a name, `all` or `none` (default: the calling distro, else all)
        #[arg(long)]
        wsl: Option<String>,
        /// Also add the install dir to your user PATH (for cmd/PowerShell: `toastback-cli`)
        #[arg(long)]
        add_to_path: bool,
    },
    /// Remove everything `install` set up
    Uninstall {
        /// Also delete logs, pending notifications and the image cache
        #[arg(long)]
        purge: bool,
    },
    /// Print the WSL shim
    Shim {
        /// Windows path of the exe the shim runs (default: the installed copy)
        #[arg(long)]
        win_exe: Option<String>,
    },
    /// List the built-in example specs, or print one
    Examples {
        /// Example to print (save it with `>`, or pipe it into `toastback`)
        name: Option<String>,
    },
    /// Check specs without showing them
    Validate {
        /// JSON spec files (- for stdin)
        #[arg(required = true)]
        files: Vec<String>,
    },
    /// Check the installation: registration, notifications, WSL shim
    Doctor {
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Remove notifications from Action Center and forget them (their buttons stop working)
    Remove {
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Remove all toastback notifications
    Clear,
    /// Show recent events: notifications shown, clicks, callback results
    Log {
        /// Number of events
        #[arg(short = 'n', default_value_t = 20)]
        n: usize,
        /// Only this notification
        #[arg(long)]
        id: Option<String>,
    },
    /// Protocol handler for toastback:// URIs (internal)
    #[command(hide = true)]
    Activate { uri: String },
    /// Show the dialog for a pending notification (internal, started detached by `show`)
    #[command(name = "__dialog", hide = true)]
    Dialog { id: String },
}

#[cfg(not(windows))]
pub fn run(_cli: Cli) -> Result<u8> {
    bail!("toastback only runs on Windows")
}

#[cfg(windows)]
pub fn run(cli: Cli) -> Result<u8> {
    use crate::store::{Store, unix_now};

    let store = Store::open()?;
    let ctx = cli.ctx.context();
    if let Ok(pruned) = store.prune(unix_now())
        && !pruned.is_empty()
    {
        let _ = store.log("pruned", serde_json::json!({ "ids": pruned }));
    }
    match cli.command {
        Command::Show(args) => Ok(show::show(&store, &args, &ctx)),
        Command::Install { wsl, add_to_path } => {
            let opts = install::Options {
                wsl: wsl.as_deref(),
                add_to_path,
            };
            install::install(&store, &opts, &ctx, env!("CARGO_PKG_VERSION"))?;
            let spec = Spec {
                title: "toastback installed".into(),
                body: Some(format!("Version {VERSION}. Notifications work.")),
                ..Default::default()
            };
            match show::show_spec(&store, spec) {
                Ok(_) => outln!("done: a test notification is on its way"),
                Err((_, e)) => errln!("warning: test notification failed: {e:#}"),
            }
            Ok(0)
        }
        Command::Uninstall { purge } => {
            install::uninstall(&store, purge)?;
            Ok(0)
        }
        Command::Shim { win_exe } => {
            let exe = win_exe.unwrap_or_else(|| store.installed_exe().to_string_lossy().into());
            out!("{}", install::render_shim(env!("CARGO_PKG_VERSION"), &exe));
            Ok(0)
        }
        Command::Examples { name: None } => {
            out!("{}", examples::list());
            Ok(0)
        }
        Command::Examples { name: Some(name) } => match examples::find(&name) {
            Some(e) => {
                out!("{}", e.json);
                Ok(0)
            }
            None => {
                errln!("toastback: no example {name:?}. The examples are:\n");
                errln!("{}", examples::list().trim_end());
                Ok(show::EXIT_INVALID)
            }
        },
        Command::Validate { files } => {
            let mut bad = 0;
            for file in &files {
                match show::validate_file(file, &ctx) {
                    Ok(()) => outln!("ok    {file}"),
                    Err(e) => {
                        bad += 1;
                        outln!("FAIL  {file}: {e:#}");
                    }
                }
            }
            Ok(if bad > 0 { show::EXIT_INVALID } else { 0 })
        }
        Command::Doctor { json } => {
            let checks = doctor::run(&store, &ctx, env!("CARGO_PKG_VERSION"));
            if json {
                outln!("{}", serde_json::to_string_pretty(&checks)?);
            } else {
                out!("{}", doctor::render(&checks));
            }
            Ok(doctor::exit_code(&checks))
        }
        Command::Remove { ids } => {
            let mut missing = 0;
            for id in &ids {
                // by file, not by content: one written by a newer version can be removed too
                if store.has_pending(id) {
                    let _ = toast::remove(id);
                    store.remove_pending(id)?;
                    store.log("removed", serde_json::json!({ "id": id }))?;
                    outln!("removed {id}");
                } else {
                    missing += 1;
                    errln!("toastback: no notification {id}");
                }
            }
            Ok(u8::from(missing > 0))
        }
        Command::Clear => {
            let _ = toast::remove_all();
            let ids = store.pending_ids()?;
            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
            store.remove_pendings(&ids)?;
            store.log("cleared", serde_json::json!({ "ids": ids }))?;
            outln!("removed {} notification(s)", ids.len());
            Ok(0)
        }
        Command::Log { n, id } => {
            let events: Vec<Value> = store
                .read_log()?
                .into_iter()
                .filter(|e| id.is_none() || e["id"].as_str() == id.as_deref())
                .collect();
            for e in &events[events.len().saturating_sub(n)..] {
                out!("{}", format_event(e));
            }
            Ok(0)
        }
        Command::Dialog { id } => {
            if let Err(e) = activate::dialog(&store, &id) {
                let _ = store.log(
                    "error",
                    serde_json::json!({ "id": id, "error": format!("{e:#}") }),
                );
            }
            Ok(0)
        }
        Command::Activate { uri } => {
            if let Err(e) = activate::handle(&store, &uri) {
                let _ = store.log(
                    "error",
                    serde_json::json!({ "uri": uri, "error": format!("{e:#}") }),
                );
            }
            Ok(0)
        }
    }
}

/// One log event as text: `ts  event  id  details`, then indented stdout/stderr tails.
fn format_event(e: &Value) -> String {
    let s = |k: &str| e[k].as_str().unwrap_or("").to_string();
    let id = Some(s("id")).filter(|i| !i.is_empty());
    let mut line = format!(
        "{}  {:<17}  {}",
        s("ts"),
        s("event"),
        id.as_deref().unwrap_or("-")
    );
    let mut extra = Vec::new();
    for key in ["action", "title", "reason", "error", "uri"] {
        if let Some(v) = e[key].as_str() {
            extra.push(if key == "title" {
                format!("{v:?}")
            } else {
                v.to_string()
            });
        }
    }
    if let Some(ids) = e["ids"].as_array() {
        extra.push(format!("{} ids", ids.len()));
    }
    if e["event"] == "callback_finished" {
        if e["timed_out"].as_bool() == Some(true) {
            extra.push("timed out".into());
        } else {
            extra.push(format!("exit {}", e["exit_code"]));
        }
        if let Some(ms) = e["duration_ms"].as_u64() {
            extra.push(format!("{:.1}s", ms as f64 / 1000.0));
        }
    }
    if !extra.is_empty() {
        line += "  ";
        line += &extra.join("  ");
    }
    line.push('\n');
    for stream in ["stdout", "stderr"] {
        for l in e[stream].as_str().unwrap_or("").lines() {
            line += &format!("    {stream} | {l}\n");
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn help_shows_examples() {
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        let top = cmd.render_help().to_string();
        let show = cmd
            .find_subcommand_mut("show")
            .unwrap()
            .render_help()
            .to_string();
        for help in [top, show] {
            assert!(
                help.contains(r#"toastback -t "Hello" -m "World""#),
                "{help}"
            );
            assert!(help.contains("toastback examples "), "{help}");
        }
    }

    #[test]
    fn show_is_the_default_and_context_is_global() {
        let parse = |args: &[&str]| {
            Cli::parse_with_default(std::iter::once("toastback").chain(args.iter().copied()))
                .unwrap()
        };
        let show = |cli: &Cli| match &cli.command {
            Command::Show(a) => a.clone(),
            _ => panic!("not show"),
        };

        let cli = parse(&[
            "--distro", "D", "--user", "u", "--cwd", "/w", "-t", "hi", "-m", "x",
        ]);
        assert_eq!(show(&cli).title.as_deref(), Some("hi"));
        assert_eq!(cli.ctx.context().distro.as_deref(), Some("D"));

        let cli = parse(&["--distro", "D", "install", "--wsl", "none"]);
        assert!(
            matches!(cli.command, Command::Install { wsl: Some(ref w), add_to_path: false } if w == "none")
        );
        assert_eq!(cli.ctx.context().distro.as_deref(), Some("D"));

        let cli = parse(&["--distro=D", "examples/deploy.json"]);
        assert_eq!(show(&cli).file.as_deref(), Some("examples/deploy.json"));

        let cli = parse(&["--distro=D", "examples", "deploy"]);
        assert!(matches!(cli.command, Command::Examples { name: Some(ref n) } if n == "deploy"));
        let cli = parse(&["examples"]);
        assert!(matches!(cli.command, Command::Examples { name: None }));
        let cli = parse(&["./examples"]);
        assert_eq!(show(&cli).file.as_deref(), Some("./examples"));

        let cli = parse(&["show", "-", "--mode", "dialog"]);
        assert_eq!(show(&cli).file.as_deref(), Some("-"));
        assert_eq!(show(&cli).mode, Some(Mode::Dialog));

        // a missing value doesn't swallow the next flag
        assert!(Cli::parse_with_default(["toastback", "-t", "-m", "x"]).is_err());
        // text from a script may start with a dash
        let cli = parse(&[
            "-t",
            "cmake",
            "-m",
            "-- Build files have been written",
            "-d",
            "-1",
        ]);
        assert_eq!(
            show(&cli).body.as_deref(),
            Some("-- Build files have been written")
        );
        assert_eq!(show(&cli).details.as_deref(), Some("-1"));

        // no arguments at all: show, reading stdin
        assert_eq!(show(&parse(&[])).file, None);
        // context flags after the subcommand work too (global)
        let cli = parse(&["log", "--distro", "D"]);
        assert_eq!(cli.ctx.context().distro.as_deref(), Some("D"));
    }

    #[test]
    fn load_spec_merges_flags_over_file_context() {
        let args = ShowArgs {
            title: Some("hi".into()),
            ..Default::default()
        };
        let ctx = Context {
            distro: Some("D".into()),
            ..Default::default()
        };
        let spec = crate::show::load_spec(&args, &ctx).unwrap();
        assert_eq!(spec.title, "hi");
        assert_eq!(spec.context.distro.as_deref(), Some("D"));
    }

    #[test]
    fn formats_callback_events() {
        let e = serde_json::json!({
            "ts": "2026-10-14T13:00:00Z", "event": "callback_finished", "id": "x1",
            "action": "deploy", "exit_code": 0, "timed_out": false, "duration_ms": 1234,
            "stdout": "line 1\nline 2", "stderr": ""
        });
        assert_eq!(
            format_event(&e),
            "2026-10-14T13:00:00Z  callback_finished  x1  deploy  exit 0  1.2s\n    stdout | line 1\n    stdout | line 2\n"
        );
    }
}
