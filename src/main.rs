// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg_attr(windows, windows_subsystem = "windows")]
// Most logic is platform-neutral and unit-tested on the Linux host, where only the tests use it.
#![cfg_attr(not(windows), allow(dead_code))]

mod activate;
#[cfg(windows)]
mod console;
mod context;
mod dialog;
mod exec;
mod install;
mod spec;
mod store;
mod toast;

use std::io::{IsTerminal as _, Read as _};
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use clap::{Args, Parser, Subcommand};

use serde_json::Value;

use crate::spec::{Context, Mode, QuickFlags, Scenario, Spec};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TOASTBACK_GIT_SHA"),
    ")"
);

/// Exit codes of `show` (PLAN.md §5).
const EXIT_INVALID: u8 = 2;
const EXIT_SHOW_FAILED: u8 = 3;
const EXIT_NOT_INSTALLED: u8 = 4;

/// Async Windows toasts with callbacks, for WSL and scripts.
///
/// Shows a notification described by a JSON spec (FILE, or stdin) and/or quick flags, prints its
/// id and exits. Clicking a button later runs that button's callback.
#[derive(Parser)]
#[command(version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    #[command(flatten)]
    ctx: CtxArgs,
}

impl Cli {
    /// Parse, with `show` as the default command: `toastback [ctx flags] -t hi` means
    /// `toastback [ctx flags] show -t hi`.
    fn parse_with_default<I: Into<std::ffi::OsString>>(
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
        Cli::try_parse_from(args)
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
struct ShowArgs {
    /// JSON spec file, or - for stdin (default: stdin, unless -t/-m are given)
    file: Option<String>,
    /// Title (overrides the spec)
    #[arg(short = 't', long)]
    title: Option<String>,
    /// Body text
    #[arg(short = 'm', long = "body")]
    body: Option<String>,
    /// Details, shown in a dialog when the toast body is clicked
    #[arg(short = 'd', long)]
    details: Option<String>,
    /// Icon (PNG/JPG; Windows or WSL path)
    #[arg(short = 'i', long)]
    icon: Option<String>,
    #[arg(long, value_enum)]
    mode: Option<Mode>,
    #[arg(long, value_enum)]
    scenario: Option<Scenario>,
    /// Notification id (default: generated)
    #[arg(long)]
    id: Option<String>,
}

impl ShowArgs {
    fn flags(&self) -> QuickFlags {
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
    Show(ShowArgs),
    /// Install for the current user: copy the exe, register it, write the WSL shim
    Install {
        /// Distro(s) to write the shim to: a name, `all` or `none` (default: the calling distro, else all)
        #[arg(long)]
        wsl: Option<String>,
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

fn main() -> ExitCode {
    // The protocol handler and the dialog process have no terminal; anything else may have one.
    // The guard restores the console mode on drop, so every path below must return, not exit().
    #[cfg(windows)]
    let _console = std::env::args_os()
        .nth(1)
        .is_none_or(|a| a != "activate" && a != "__dialog")
        .then(console::attach_missing_streams);
    let cli = match Cli::parse_with_default(std::env::args_os()) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(e.exit_code() as u8);
        }
    };
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("toastback: {e:#}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(windows))]
fn run(_cli: Cli) -> Result<u8> {
    bail!("toastback only runs on Windows")
}

#[cfg(windows)]
fn run(cli: Cli) -> Result<u8> {
    use crate::store::{Store, unix_now};

    let store = Store::open()?;
    let ctx = cli.ctx.context();
    if let Ok(pruned) = store.prune(unix_now())
        && !pruned.is_empty()
    {
        let _ = store.log("pruned", serde_json::json!({ "ids": pruned }));
    }
    match cli.command {
        Command::Show(args) => Ok(show(&store, &args, &ctx)),
        Command::Install { wsl } => {
            install::install(&store, wsl.as_deref(), &ctx, env!("CARGO_PKG_VERSION"))?;
            let spec = Spec {
                title: "toastback installed".into(),
                body: Some(format!("Version {VERSION}. Notifications work.")),
                ..Default::default()
            };
            match show_spec(&store, spec) {
                Ok(_) => println!("done: a test notification is on its way"),
                Err((_, e)) => eprintln!("warning: test notification failed: {e:#}"),
            }
            Ok(0)
        }
        Command::Uninstall { purge } => {
            install::uninstall(&store, purge)?;
            Ok(0)
        }
        Command::Shim { win_exe } => {
            let exe = win_exe.unwrap_or_else(|| store.installed_exe().to_string_lossy().into());
            print!("{}", install::render_shim(env!("CARGO_PKG_VERSION"), &exe));
            Ok(0)
        }
        Command::Log { n, id } => {
            let events: Vec<Value> = store
                .read_log()?
                .into_iter()
                .filter(|e| id.is_none() || e["id"].as_str() == id.as_deref())
                .collect();
            for e in &events[events.len().saturating_sub(n)..] {
                print!("{}", format_event(e));
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

/// Read the spec, merge flags and context, validate, show. Prints the id; returns the exit code.
#[cfg(windows)]
fn show(store: &store::Store, args: &ShowArgs, ctx: &Context) -> u8 {
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

fn load_spec(args: &ShowArgs, ctx: &Context) -> Result<Spec> {
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
fn show_spec(store: &store::Store, mut spec: Spec) -> Result<String, (u8, anyhow::Error)> {
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
            let spawned = std::env::current_exe()
                .map_err(anyhow::Error::from)
                .and_then(|exe| dialog::spawn_detached(&exe, &["__dialog", &id]));
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
        assert!(matches!(cli.command, Command::Install { wsl: Some(ref w) } if w == "none"));
        assert_eq!(cli.ctx.context().distro.as_deref(), Some("D"));

        let cli = parse(&["--distro=D", "examples/deploy.json"]);
        assert_eq!(show(&cli).file.as_deref(), Some("examples/deploy.json"));

        let cli = parse(&["show", "-", "--mode", "dialog"]);
        assert_eq!(show(&cli).file.as_deref(), Some("-"));
        assert_eq!(show(&cli).mode, Some(Mode::Dialog));

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
        let spec = load_spec(&args, &ctx).unwrap();
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
