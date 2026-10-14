// SPDX-License-Identifier: GPL-3.0-or-later
//! Callbacks: turn a `run` into a process plan (pure, tested on the host), then run it (Windows).

use std::time::Duration;

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::context;
use crate::spec::{Context, Run, RunKind};

/// Output kept per stream for the log.
pub const TAIL_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Process {
        program: String,
        /// Tried when `program` isn't installed (`pwsh` → `powershell.exe`).
        fallback: Option<String>,
        args: Vec<String>,
        /// Appended verbatim to the command line (cmd.exe has its own quoting rules).
        raw_args: Option<String>,
        env: Vec<(String, String)>,
        /// `None`: the user's profile dir.
        cwd: Option<String>,
    },
    Open {
        target: String,
        cwd: Option<String>,
    },
}

impl Plan {
    /// For the log: what runs and where, with the names of the environment variables but not their
    /// values (an `env` value may be a token).
    pub fn describe(&self) -> Value {
        match self {
            Plan::Process {
                program,
                fallback,
                args,
                raw_args,
                env,
                cwd,
            } => json!({
                "program": program, "fallback": fallback, "args": args, "raw_args": raw_args,
                "cwd": cwd, "env": env.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            }),
            Plan::Open { target, cwd } => json!({ "open": target, "cwd": cwd }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    /// `None` when killed after the timeout.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration: Duration,
    pub stdout: String,
    pub stderr: String,
}

/// Build the process for a callback.
///
/// `vars` are the `TOASTBACK_*` variables. For `wsl` runs, they and the run's own `env` are added to
/// `WSLENV` (`/u`: Windows → Linux) so they cross into the distro; `wslenv` is the current value.
pub fn plan(
    run: &Run,
    ctx: &Context,
    vars: &[(String, String)],
    wslenv: Option<&str>,
) -> Result<Plan> {
    let mut env: Vec<(String, String)> = vars.to_vec();
    env.extend(run.env.iter().map(|(k, v)| (k.clone(), v.clone())));

    let windows_cwd = || -> Result<Option<String>> {
        let Some(c) = run.cwd.as_deref() else {
            return Ok(None);
        };
        let win = context::to_windows_path(c, ctx)?;
        if !context::is_windows_path(&win) {
            bail!("cwd {c:?} is relative, and there's no caller directory to resolve it against");
        }
        Ok(Some(win))
    };
    let process = |program: &str, args: Vec<String>, env, cwd| Plan::Process {
        program: program.into(),
        fallback: None,
        args,
        raw_args: None,
        env,
        cwd,
    };

    Ok(match run.kind()? {
        RunKind::WslShell(_) | RunKind::WslArgv(_) => {
            // The caller's user and directory belong to the caller's distro: a run in another one
            // gets that distro's default user and the home directory, unless it sets them.
            let other_distro = run.distro.as_ref().is_some_and(|d| {
                ctx.distro
                    .as_ref()
                    .is_none_or(|c| !c.eq_ignore_ascii_case(d))
            });
            let mut args = Vec::new();
            if let Some(d) = run.distro.as_ref().or(ctx.distro.as_ref()) {
                args.extend(["-d".into(), d.clone()]);
            }
            let user = run
                .user
                .as_ref()
                .or(ctx.user.as_ref().filter(|_| !other_distro));
            if let Some(u) = user {
                args.extend(["-u".into(), u.clone()]);
            }
            let base = ctx.cwd.as_deref().filter(|_| !other_distro);
            args.extend(["--cd".into(), wsl_cwd(run.cwd.as_deref(), base)?]);
            // --exec: wsl.exe passes the argv as is. `--` would hand the rest of the command line
            // to the user's login shell for a second round of parsing.
            args.push("--exec".into());
            match run.kind()? {
                RunKind::WslShell(s) => args.extend(["bash".into(), "-lc".into(), s.into()]),
                RunKind::WslArgv(a) => args.extend(a.iter().cloned()),
                _ => unreachable!(),
            }
            let names: Vec<String> = env.iter().map(|(k, _)| format!("{k}/u")).collect();
            let wslenv = match wslenv.filter(|w| !w.is_empty()) {
                Some(w) => format!("{w}:{}", names.join(":")),
                None => names.join(":"),
            };
            env.push(("WSLENV".into(), wslenv));
            // wsl.exe's own messages ("no such distro", ...) in UTF-8, not UTF-16, for the log
            env.push(("WSL_UTF8".into(), "1".into()));
            process("wsl.exe", args, env, None)
        }
        RunKind::Cmd(s) => {
            let cwd = windows_cwd()?;
            if let Some(c) = cwd.as_deref().filter(|c| c.starts_with(r"\\")) {
                // cmd.exe would print a warning and run the command in C:\Windows instead
                bail!(
                    "cmd can't run in {c}: cmd.exe doesn't accept UNC directories (WSL paths outside \
                     /mnt/<drive>). Use a Windows directory as cwd, or pwsh"
                );
            }
            Plan::Process {
                program: "cmd.exe".into(),
                fallback: None,
                args: Vec::new(),
                raw_args: Some(format!("/d /s /c \"{s}\"")),
                env,
                cwd,
            }
        }
        RunKind::Pwsh(s) => Plan::Process {
            program: "pwsh.exe".into(),
            fallback: Some("powershell.exe".into()),
            // -EncodedCommand (base64 UTF-16LE) sidesteps every quoting rule
            args: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-EncodedCommand".into(),
                base64_utf16le(s),
            ],
            raw_args: None,
            env,
            cwd: windows_cwd()?,
        },
        RunKind::Exe(argv) => process(&argv[0], argv[1..].to_vec(), env, windows_cwd()?),
        RunKind::Open(target) => Plan::Open {
            target: target.into(),
            cwd: windows_cwd()?,
        },
    })
}

/// Check at show time what `plan` would reject at click time (a relative cwd with no caller
/// directory, cmd with a UNC cwd), so the error reaches the caller instead of the log.
pub fn check(run: &Run, ctx: &Context) -> Result<()> {
    plan(run, ctx, &[], None).map(drop)
}

/// The `--cd` for wsl.exe, which takes only `~`, absolute Linux paths and Windows paths: a relative
/// `cwd` is resolved against the caller's directory.
fn wsl_cwd(cwd: Option<&str>, base: Option<&str>) -> Result<String> {
    let absolute =
        |c: &str| c.starts_with('/') || c.starts_with('~') || context::is_windows_path(c);
    let base = base.filter(|b| absolute(b));
    Ok(match (cwd, base) {
        (Some(c), _) if absolute(c) => c.to_string(),
        (Some(c), Some(b)) if b.starts_with('/') => format!("{}/{c}", b.trim_end_matches('/')),
        (Some(c), _) => {
            bail!("cwd {c:?} is relative, and there's no caller directory to resolve it against")
        }
        (None, Some(b)) => b.to_string(),
        (None, None) => "~".into(),
    })
}

/// `scheme:…` with a scheme of two or more characters (so not `C:\…`): `https://…`, `mailto:…`.
pub fn is_url(target: &str) -> bool {
    target.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() >= 2
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

fn base64_utf16le(s: &str) -> String {
    let bytes: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The last `TAIL_BYTES` of a stream, as (lossy) UTF-8.
pub fn tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(TAIL_BYTES);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(windows)]
pub use imp::execute;

#[cfg(windows)]
mod imp {
    use std::io::Read;
    use std::os::windows::io::AsRawHandle as _;
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    use anyhow::{Context as _, Result, bail};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    use super::{Outcome, Plan, TAIL_BYTES, is_url, tail};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// After the callback exits, how long to wait for programs it started that still hold its
    /// stdout/stderr (`start notepad`, a server in the background).
    const OUTPUT_GRACE: Duration = Duration::from_secs(2);

    pub fn execute(plan: &Plan, timeout: Option<Duration>) -> Result<Outcome> {
        let start = Instant::now();
        match plan {
            Plan::Open { target, cwd } => {
                let dir = cwd.as_deref().map(HSTRING::from);
                // %VAR%s in paths, not URLs: `?q=%OS%` must stay as it is
                let target = if is_url(target) {
                    target.clone()
                } else {
                    expand_env(target)
                };
                // SAFETY: all strings are NUL-terminated HSTRINGs that outlive the call.
                let h = unsafe {
                    ShellExecuteW(
                        None,
                        w!("open"),
                        &HSTRING::from(&target),
                        PCWSTR::null(),
                        dir.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
                        SW_SHOWNORMAL,
                    )
                };
                if h.0 as usize <= 32 {
                    bail!(
                        "ShellExecute failed to open {target:?} (code {})",
                        h.0 as usize
                    );
                }
                Ok(Outcome {
                    exit_code: Some(0),
                    timed_out: false,
                    duration: start.elapsed(),
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
            Plan::Process {
                program,
                fallback,
                args,
                raw_args,
                env,
                cwd,
            } => {
                let spawn = |program: &str| {
                    let mut c = Command::new(program);
                    c.args(args);
                    if let Some(raw) = raw_args {
                        c.raw_arg(raw);
                    }
                    c.envs(env.iter().map(|(k, v)| (k, v)));
                    match cwd {
                        Some(d) => {
                            c.current_dir(d);
                        }
                        None => {
                            if let Some(home) = std::env::var_os("USERPROFILE") {
                                c.current_dir(home);
                            }
                        }
                    }
                    c.stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .creation_flags(CREATE_NO_WINDOW)
                        .spawn()
                };
                let mut child = match (spawn(program), fallback) {
                    (Err(e), Some(fb)) if e.kind() == std::io::ErrorKind::NotFound => {
                        spawn(fb).with_context(|| format!("start {fb} (fallback for {program})"))?
                    }
                    (r, _) => r.with_context(|| format!("start {program}"))?,
                };
                // Everything the callback starts joins its job, so a timeout ends all of it, not
                // just cmd.exe or pwsh.exe. (A program started in the moment before the
                // assignment escapes the job; Command can't start the child suspended.)
                let job = Job::new().ok().filter(|j| j.assign(&child));
                let out = Drain::start(child.stdout.take().unwrap());
                let err = Drain::start(child.stderr.take().unwrap());
                let mut timed_out = false;
                let status = loop {
                    if let Some(s) = child.try_wait()? {
                        break Some(s);
                    }
                    if timeout.is_some_and(|t| start.elapsed() >= t) {
                        if let Some(job) = &job {
                            job.terminate();
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break None;
                    }
                    thread::sleep(Duration::from_millis(50));
                };
                let duration = start.elapsed();
                let deadline = Instant::now() + OUTPUT_GRACE;
                Ok(Outcome {
                    exit_code: status.and_then(|s| s.code()),
                    timed_out,
                    duration,
                    stdout: out.collect(deadline),
                    stderr: err.collect(deadline),
                })
            }
        }
    }

    /// A job object for one callback's process tree. Closing it leaves the processes running:
    /// `start notepad` from a callback keeps its window.
    struct Job(HANDLE);

    impl Job {
        fn new() -> Result<Job> {
            // SAFETY: no security attributes, no name; the handle is owned by the Job and closed
            // on drop.
            Ok(Job(unsafe { CreateJobObjectW(None, PCWSTR::null()) }?))
        }

        fn assign(&self, child: &std::process::Child) -> bool {
            // SAFETY: both handles are valid for the call; the child's stays owned by `child`.
            unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }.is_ok()
        }

        fn terminate(&self) {
            // SAFETY: valid job handle.
            let _ = unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle came from CreateJobObjectW and is closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    /// Expand `%VAR%` references (ShellExecute doesn't). Unknown variables stay as they are.
    fn expand_env(s: &str) -> String {
        let src = HSTRING::from(s);
        // SAFETY: first call only asks for the size; the buffer then has exactly that many units.
        unsafe {
            let n = ExpandEnvironmentStringsW(&src, None);
            if n == 0 {
                return s.to_string();
            }
            let mut buf = vec![0u16; n as usize];
            let written = ExpandEnvironmentStringsW(&src, Some(&mut buf));
            if written == 0 || written > n {
                return s.to_string();
            }
            String::from_utf16_lossy(&buf[..written as usize - 1])
        }
    }

    /// Drains a pipe on a thread, keeping only the last `TAIL_BYTES` (plus one read's worth).
    struct Drain {
        kept: Arc<Mutex<Vec<u8>>>,
        done: mpsc::Receiver<()>,
    }

    impl Drain {
        fn start(mut r: impl Read + Send + 'static) -> Drain {
            let kept = Arc::new(Mutex::new(Vec::new()));
            let (tx, done) = mpsc::channel();
            let shared = kept.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut kept = shared.lock().unwrap_or_else(|e| e.into_inner());
                    kept.extend_from_slice(&buf[..n]);
                    if kept.len() > 2 * TAIL_BYTES {
                        let cut = kept.len() - TAIL_BYTES;
                        kept.drain(..cut);
                    }
                }
                let _ = tx.send(());
            });
            Drain { kept, done }
        }

        /// The output so far, once the pipe closes or at `deadline`, whichever is first. A
        /// program the callback left running may hold the pipe open for good.
        fn collect(&self, deadline: Instant) -> String {
            let _ = self
                .done
                .recv_timeout(deadline.saturating_duration_since(Instant::now()));
            tail(&self.kept.lock().unwrap_or_else(|e| e.into_inner()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Spec;

    fn run(json: &str) -> Run {
        let s = Spec::parse(&format!(r#"{{"title": "t", "default": {json}}}"#)).unwrap();
        s.validate().unwrap();
        s.default.unwrap()
    }

    fn ctx() -> Context {
        Context {
            distro: Some("Ubuntu".into()),
            user: Some("g".into()),
            cwd: Some("/home/g/app".into()),
        }
    }

    fn vars() -> Vec<(String, String)> {
        vec![("TOASTBACK_ID".into(), "x1".into())]
    }

    #[test]
    fn wsl_shell() {
        let p = plan(
            &run(r#"{"wsl": "./deploy.sh --env prod", "env": {"FOO": "bar"}}"#),
            &ctx(),
            &vars(),
            Some("PATH/l"),
        )
        .unwrap();
        let Plan::Process {
            program, args, env, ..
        } = p
        else {
            panic!()
        };
        assert_eq!(program, "wsl.exe");
        assert_eq!(
            args,
            [
                "-d",
                "Ubuntu",
                "-u",
                "g",
                "--cd",
                "/home/g/app",
                "--exec",
                "bash",
                "-lc",
                "./deploy.sh --env prod"
            ]
        );
        assert!(env.contains(&("FOO".into(), "bar".into())));
        assert!(env.contains(&("WSLENV".into(), "PATH/l:TOASTBACK_ID/u:FOO/u".into())));
    }

    #[test]
    fn wsl_argv_with_overrides_and_no_context() {
        let p = plan(
            &run(r#"{"wsl": ["ls", "-la"], "distro": "Debian", "cwd": "/tmp"}"#),
            &Context::default(),
            &vars(),
            None,
        )
        .unwrap();
        let Plan::Process { args, env, .. } = p else {
            panic!()
        };
        assert_eq!(
            args,
            ["-d", "Debian", "--cd", "/tmp", "--exec", "ls", "-la"]
        );
        assert!(env.contains(&("WSLENV".into(), "TOASTBACK_ID/u".into())));
    }

    #[test]
    fn other_distro_gets_its_own_user_and_home() {
        let args = |json: &str, ctx: &Context| match plan(&run(json), ctx, &vars(), None).unwrap() {
            Plan::Process { args, .. } => args,
            p => panic!("{p:?}"),
        };
        assert_eq!(
            args(r#"{"wsl": ["id"], "distro": "Debian"}"#, &ctx()),
            ["-d", "Debian", "--cd", "~", "--exec", "id"]
        );
        // the caller's own distro (in another case): the caller's user and directory
        assert_eq!(
            args(r#"{"wsl": ["id"], "distro": "ubuntu"}"#, &ctx()),
            [
                "-d",
                "ubuntu",
                "-u",
                "g",
                "--cd",
                "/home/g/app",
                "--exec",
                "id"
            ]
        );
        assert_eq!(
            args(
                r#"{"wsl": ["id"], "distro": "Debian", "user": "root", "cwd": "/srv"}"#,
                &ctx()
            ),
            ["-d", "Debian", "-u", "root", "--cd", "/srv", "--exec", "id"]
        );
    }

    #[test]
    fn relative_cwd_resolves_against_the_caller() {
        let cd = |json: &str, ctx: &Context| {
            plan(&run(json), ctx, &vars(), None).map(|p| match p {
                Plan::Process { args, cwd, .. } => {
                    let i = args.iter().position(|a| a == "--cd");
                    i.map_or(cwd, |i| Some(args[i + 1].clone()))
                }
                p => panic!("{p:?}"),
            })
        };
        let wsl = r#"{"wsl": "x", "cwd": "scripts"}"#;
        assert_eq!(cd(wsl, &ctx()).unwrap().unwrap(), "/home/g/app/scripts");
        assert!(cd(wsl, &Context::default()).is_err());
        assert_eq!(
            cd(r#"{"wsl": "x", "cwd": "~/src"}"#, &Context::default())
                .unwrap()
                .unwrap(),
            "~/src"
        );
        // Windows forms: through the caller's directory, so a UNC path
        assert_eq!(
            cd(r#"{"pwsh": "x", "cwd": "scripts"}"#, &ctx())
                .unwrap()
                .unwrap(),
            r"\\wsl.localhost\Ubuntu\home\g\app\scripts"
        );
        assert!(cd(r#"{"pwsh": "x", "cwd": "scripts"}"#, &Context::default()).is_err());
    }

    #[test]
    fn cmd_refuses_unc_cwd() {
        let r = run(r#"{"cmd": "del /q *.log", "cwd": "/home/g/build"}"#);
        let e = plan(&r, &ctx(), &vars(), None).unwrap_err().to_string();
        assert!(e.contains("UNC"), "{e}");
        assert!(check(&r, &ctx()).is_err());
        check(&run(r#"{"cmd": "dir", "cwd": "/mnt/c/build"}"#), &ctx()).unwrap();
    }

    #[test]
    fn describe_leaves_out_env_values() {
        let p = plan(
            &run(r#"{"wsl": "gh release upload", "env": {"GH_TOKEN": "ghp_secret"}}"#),
            &ctx(),
            &vars(),
            None,
        )
        .unwrap();
        let d = p.describe().to_string();
        assert!(!d.contains("ghp_secret"), "{d}");
        assert!(d.contains("GH_TOKEN"), "{d}");
        assert!(d.contains("gh release upload"), "{d}");
    }

    #[test]
    fn urls_and_paths() {
        for url in [
            "https://x.example/?q=%OS%",
            "mailto:a@b.c",
            "ms-settings:display",
        ] {
            assert!(is_url(url), "{url}");
        }
        for path in [
            r"C:\Users\%USERNAME%",
            "%USERPROFILE%\\x",
            r"\\srv\share",
            "notes.txt",
        ] {
            assert!(!is_url(path), "{path}");
        }
    }

    #[test]
    fn windows_forms() {
        let p = plan(&run(r#"{"cmd": "echo a & echo b"}"#), &ctx(), &vars(), None).unwrap();
        assert!(
            matches!(&p, Plan::Process { program, raw_args: Some(r), cwd: None, .. }
            if program == "cmd.exe" && r == "/d /s /c \"echo a & echo b\"")
        );

        let p = plan(
            &run(r#"{"pwsh": "Write-Output 'hi'"}"#),
            &ctx(),
            &vars(),
            None,
        )
        .unwrap();
        let Plan::Process {
            program,
            fallback,
            args,
            ..
        } = p
        else {
            panic!()
        };
        assert_eq!(program, "pwsh.exe");
        assert_eq!(fallback.as_deref(), Some("powershell.exe"));
        assert_eq!(args[3], base64_utf16le("Write-Output 'hi'"));

        let p = plan(
            &run(r#"{"exe": ["C:\\x.exe", "a b"], "cwd": "/mnt/c/tmp"}"#),
            &ctx(),
            &vars(),
            None,
        )
        .unwrap();
        assert!(
            matches!(&p, Plan::Process { program, args, cwd: Some(c), .. }
            if program == r"C:\x.exe" && args == &["a b"] && c == r"C:\tmp")
        );

        let p = plan(
            &run(r#"{"open": "https://example.com"}"#),
            &ctx(),
            &vars(),
            None,
        )
        .unwrap();
        assert_eq!(
            p,
            Plan::Open {
                target: "https://example.com".into(),
                cwd: None
            }
        );
    }

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // what PowerShell's [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes('dir')) gives
        assert_eq!(base64_utf16le("dir"), "ZABpAHIA");
    }

    #[test]
    fn tail_keeps_the_end() {
        let big = vec![b'a'; TAIL_BYTES + 10];
        assert_eq!(tail(&big).len(), TAIL_BYTES);
        assert_eq!(tail(b"short"), "short");
    }
}
