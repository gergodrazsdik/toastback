// SPDX-License-Identifier: GPL-3.0-or-later
//! Callbacks: turn a `run` into a process plan (pure, tested on the host), then run it (Windows).

use std::time::Duration;

use anyhow::Result;

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
        run.cwd
            .as_deref()
            .map(|c| context::to_windows_path(c, ctx))
            .transpose()
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
            let mut args = Vec::new();
            if let Some(d) = run.distro.as_ref().or(ctx.distro.as_ref()) {
                args.extend(["-d".into(), d.clone()]);
            }
            if let Some(u) = run.user.as_ref().or(ctx.user.as_ref()) {
                args.extend(["-u".into(), u.clone()]);
            }
            let cwd = run.cwd.as_ref().or(ctx.cwd.as_ref());
            args.extend(["--cd".into(), cwd.cloned().unwrap_or_else(|| "~".into())]);
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
            process("wsl.exe", args, env, None)
        }
        RunKind::Cmd(s) => Plan::Process {
            program: "cmd.exe".into(),
            fallback: None,
            args: Vec::new(),
            raw_args: Some(format!("/d /s /c \"{s}\"")),
            env,
            cwd: windows_cwd()?,
        },
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
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use anyhow::{Context as _, Result, bail};
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    use super::{Outcome, Plan, TAIL_BYTES, tail};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn execute(plan: &Plan, timeout: Option<Duration>) -> Result<Outcome> {
        let start = Instant::now();
        match plan {
            Plan::Open { target, cwd } => {
                let dir = cwd.as_deref().map(HSTRING::from);
                // SAFETY: all strings are NUL-terminated HSTRINGs that outlive the call.
                let h = unsafe {
                    ShellExecuteW(
                        None,
                        w!("open"),
                        &HSTRING::from(expand_env(target)),
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
                let out = reader(child.stdout.take().unwrap());
                let err = reader(child.stderr.take().unwrap());
                let mut timed_out = false;
                let status = loop {
                    if let Some(s) = child.try_wait()? {
                        break Some(s);
                    }
                    if timeout.is_some_and(|t| start.elapsed() >= t) {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break None;
                    }
                    thread::sleep(Duration::from_millis(50));
                };
                Ok(Outcome {
                    exit_code: status.and_then(|s| s.code()),
                    timed_out,
                    duration: start.elapsed(),
                    stdout: tail(&out.join().unwrap_or_default()),
                    stderr: tail(&err.join().unwrap_or_default()),
                })
            }
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

    /// Drain a pipe on a thread, keeping only the last `TAIL_BYTES` (plus one read's worth).
    fn reader(mut r: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let mut kept = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = r.read(&mut buf) {
                if n == 0 {
                    break;
                }
                kept.extend_from_slice(&buf[..n]);
                if kept.len() > 2 * TAIL_BYTES {
                    kept.drain(..kept.len() - TAIL_BYTES);
                }
            }
            kept
        })
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
