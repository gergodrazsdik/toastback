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
    let path = |p: &str| program_path(p, ctx);

    let windows_cwd = || -> Result<Option<String>> {
        let Some(c) = run.cwd.as_deref() else {
            return Ok(None);
        };
        if c.starts_with('~') {
            bail!("cwd {c:?}: `~` only means the home directory for wsl callbacks");
        }
        let win = context::to_windows_path(c, ctx)?;
        if !context::is_windows_path(&win) {
            return Err(relative_cwd(c));
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
            let other_distro =
                context::is_other_distro(run.distro.as_deref(), ctx.distro.as_deref());
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
            let wslenv = merge_wslenv(&mut env, wslenv);
            env.push(("WSLENV".into(), wslenv));
            process("wsl.exe", args, env, None)
        }
        RunKind::Cmd(s) => {
            let cwd = windows_cwd()?;
            refuse_unc_for_cmd(cwd.as_deref())?;
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
        RunKind::Exe(argv) => {
            let cwd = windows_cwd()?;
            // a .bat or .cmd runs in cmd.exe too (so does cmd.exe itself)
            if runs_in_cmd(&argv[0]) {
                refuse_unc_for_cmd(cwd.as_deref())?;
            }
            process(&path(&argv[0])?, argv[1..].to_vec(), env, cwd)
        }
        RunKind::Open(target) => Plan::Open {
            target: path(target)?,
            cwd: windows_cwd()?,
        },
    })
}

/// The `WSLENV` for a `wsl` callback: the current value, then one the run's `env` sets (taken out of
/// `env`), then everything else in `env` with `/u` (Windows → Linux). A variable the run lists
/// itself keeps its own flags.
fn merge_wslenv(env: &mut Vec<(String, String)>, current: Option<&str>) -> String {
    let is_wslenv = |k: &str| k.eq_ignore_ascii_case("WSLENV");
    let own = env
        .iter()
        .find(|(k, _)| is_wslenv(k))
        .map(|(_, v)| v.clone());
    env.retain(|(k, _)| !is_wslenv(k));
    let listed = |name: &str| {
        own.iter()
            .flat_map(|o| o.split(':'))
            .any(|e| e.split('/').next() == Some(name))
    };
    let names = env
        .iter()
        .filter(|(k, _)| !listed(k))
        .map(|(k, _)| format!("{k}/u"));
    current
        .into_iter()
        .chain(own.as_deref())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .chain(names)
        .collect::<Vec<_>>()
        .join(":")
}

/// The program of an `exe` or the target of an `open`: a Linux path becomes a Windows one. Anything
/// else is left to Windows: `C:\…`, URLs, `%VAR%\…` and bare names (`notepad.exe`, found on PATH).
fn program_path(p: &str, ctx: &Context) -> Result<String> {
    if p.starts_with("./") || p.starts_with("../") {
        if ctx.cwd.is_none() {
            bail!("{p:?} is relative, and there's no caller directory to resolve it against");
        }
        return context::to_windows_path(p, ctx);
    }
    if p.starts_with('/') {
        return context::to_windows_path(p, ctx);
    }
    Ok(p.to_string())
}

/// Check at show time what `plan` would reject at click time (a relative cwd with no caller
/// directory, cmd with a UNC cwd), so the error reaches the caller instead of the log.
pub fn check(run: &Run, ctx: &Context) -> Result<()> {
    plan(run, ctx, &[], None).map(drop)
}

/// The `--cd` for wsl.exe, which takes absolute Linux and Windows paths and expands a leading `~`,
/// but resolves a relative path against its own (Windows) directory: so a relative `cwd` is
/// resolved against the caller's directory here.
fn wsl_cwd(cwd: Option<&str>, base: Option<&str>) -> Result<String> {
    let base = base.filter(|b| context::is_absolute(b));
    Ok(match (cwd, base) {
        (Some(c), _) if context::is_absolute(c) => c.to_string(),
        (Some(c), Some(b)) if b.starts_with('/') => format!("{}/{c}", b.trim_end_matches('/')),
        (Some(c), _) => return Err(relative_cwd(c)),
        (None, Some(b)) => b.to_string(),
        (None, None) => "~".into(),
    })
}

fn relative_cwd(c: &str) -> anyhow::Error {
    anyhow::anyhow!("cwd {c:?} is relative, and there's no caller directory to resolve it against")
}

/// Does `program` run in cmd.exe: cmd itself, or a batch file (which Windows hands to cmd.exe)?
fn runs_in_cmd(program: &str) -> bool {
    let name = program
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(name.as_str(), "cmd" | "cmd.exe") || name.ends_with(".bat") || name.ends_with(".cmd")
}

/// cmd.exe doesn't take a UNC directory: it would print a warning and run the command in
/// C:\Windows instead.
fn refuse_unc_for_cmd(cwd: Option<&str>) -> Result<()> {
    if let Some(c) = cwd.filter(|c| c.starts_with(r"\\")) {
        bail!(
            "cmd can't run in {c}: cmd.exe doesn't accept UNC directories (WSL paths outside \
             /mnt/<drive>). Use a Windows directory as cwd, or pwsh"
        );
    }
    Ok(())
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

/// The last `TAIL_BYTES` of a stream as text (see [`decode_output`]). The cut is at an even
/// offset, so UTF-16 stays aligned.
pub fn tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(TAIL_BYTES);
    decode_output(&bytes[start + start % 2..])
}

/// Process output as text. wsl.exe writes its own messages ("There is no distribution with the
/// supplied name", a proxy notice before the command's own output) in UTF-16LE; setting
/// `WSL_UTF8` instead would leak into every Windows program the callback starts. Both can share a
/// stream, so it's decoded piece by piece. UTF-8 has no NULs at all, and UTF-16LE text of any
/// language has them, in the high byte of its CR, LF and spaces.
///
/// Best effort where the bytes really are ambiguous: UTF-8 of even length with no line end,
/// followed by UTF-16 (`ok` and U+6B6F are the same bytes), reads as UTF-16.
pub fn decode_output(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let used = decode_piece(rest, &mut text);
        rest = &rest[used..];
    }
    text
}

/// Decode the start of `rest` into `text`, and return how many bytes that took (at least one).
fn decode_piece(rest: &[u8], text: &mut String) -> usize {
    let nul_at = |parity: usize| (0..rest.len()).filter(move |&i| i % 2 == parity && rest[i] == 0);
    if !rest.contains(&0) {
        text.push_str(&String::from_utf8_lossy(rest));
        return rest.len();
    }
    // A LF unit tells how the UTF-16 is aligned. Splitting on the LF byte alone wouldn't do: it's
    // also the low byte of U+4E0A and U+040A. Without one, most NULs are high bytes.
    let lf = rest.windows(2).position(|w| w == [b'\n', 0]);
    let start_parity = lf.map_or_else(
        || usize::from(nul_at(0).count() > nul_at(1).count()),
        |i| i % 2,
    );
    let high_parity = 1 - start_parity;
    // the high byte of the first ASCII-range character (U+4E00 has a NUL too, as its low byte)
    let Some(high) = nul_at(high_parity).next() else {
        return 1;
    };
    if high == 0 {
        return 1; // a cut one byte into UTF-16: that character is lost
    }
    let before = &rest[..high - 1];
    // Text in front of the UTF-16: it can't be aligned with it, or it's whole lines of UTF-8
    if start_parity == 1 || before.ends_with(b"\n") {
        if is_utf8_text(before) {
            text.push_str(&String::from_utf8_lossy(before));
            return before.len();
        }
        if start_parity == 1 {
            text.push_str(&String::from_utf8_lossy(&rest[..1]));
            return 1;
        }
    }
    let whole_units = rest.len() & !1;
    let end = lf.map_or_else(
        // no line end: up to the last ASCII-range character, unless what follows isn't UTF-8
        || {
            let last = nul_at(high_parity)
                .next_back()
                .map_or(whole_units, |i| i + 1);
            if is_utf8_text(&rest[last..]) {
                last
            } else {
                whole_units
            }
        },
        |i| i + 2,
    );
    text.push_str(&String::from_utf16le_lossy(&rest[..end]));
    end
}

/// Valid UTF-8 without the control characters text doesn't have (ESC: terminal colours).
fn is_utf8_text(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|s| {
        s.chars()
            .all(|c| !c.is_control() || matches!(c, '\t' | '\r' | '\n' | '\u{1b}'))
    })
}

#[cfg(windows)]
pub use imp::{Drain, OUTPUT_GRACE, execute};

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
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, Owned, PCWSTR, w};

    use super::{Outcome, Plan, TAIL_BYTES, is_url, tail};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    /// After a process exits, how long to wait for programs it started that still hold its
    /// stdout/stderr (`start notepad`, a server in the background).
    pub const OUTPUT_GRACE: Duration = Duration::from_secs(2);

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
                        // suspended until it's in the job, so nothing it starts can escape it
                        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
                        .spawn()
                };
                let mut child = match (spawn(program), fallback) {
                    (Err(e), Some(fb)) if e.kind() == std::io::ErrorKind::NotFound => {
                        spawn(fb).with_context(|| format!("start {fb} (fallback for {program})"))?
                    }
                    (r, _) => r.with_context(|| format!("start {program}"))?,
                };
                // Everything the callback starts joins its job, so a timeout ends all of it, not
                // just cmd.exe or pwsh.exe. The child starts suspended, so it can't start anything
                // before it's in the job.
                let job = Job::new().ok().filter(|j| j.assign(&child));
                if resume_threads(child.id()) == 0 {
                    let _ = child.kill();
                    bail!("start {program}: couldn't resume the new process");
                }
                let out = Drain::start(child.stdout.take().unwrap(), TAIL_BYTES);
                let err = Drain::start(child.stderr.take().unwrap(), TAIL_BYTES);
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
    struct Job(Owned<HANDLE>);

    impl Job {
        fn new() -> Result<Job> {
            // SAFETY: no security attributes, no name; Owned closes the handle on drop.
            let job = Job(unsafe { Owned::new(CreateJobObjectW(None, PCWSTR::null())?) });
            // Breakaway allowed: a dialog a callback opens (`toastback --mode dialog`) leaves the job
            // (CREATE_BREAKAWAY_FROM_JOB in dialog.rs), so the callback's timeout doesn't close it.
            // It's an extended-limit flag: the basic structure alone is refused. Without it the job
            // still does its main work, so a failure here is no reason to go without one.
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            // SAFETY: the pointer and size describe `limits`, which outlives the call.
            let _ = unsafe {
                SetInformationJobObject(
                    *job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            Ok(job)
        }

        fn assign(&self, child: &std::process::Child) -> bool {
            // SAFETY: both handles are valid for the call; the child's stays owned by `child`.
            unsafe { AssignProcessToJobObject(*self.0, HANDLE(child.as_raw_handle())) }.is_ok()
        }

        fn terminate(&self) {
            // SAFETY: valid job handle.
            let _ = unsafe { TerminateJobObject(*self.0, 1) };
        }
    }

    /// Resume the threads of a process started with CREATE_SUSPENDED (std's Child doesn't keep its
    /// main thread's handle). Returns how many were resumed.
    fn resume_threads(pid: u32) -> usize {
        let mut resumed = 0;
        // SAFETY: the snapshot and thread handles are owned (closed on drop); `entry` is a local
        // struct with its size set, as Thread32First/Next require.
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
                return 0;
            };
            let snap = Owned::new(snap);
            let mut entry = THREADENTRY32 {
                dwSize: size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut more = Thread32First(*snap, &mut entry).is_ok();
            while more {
                if entry.th32OwnerProcessID == pid
                    && let Ok(t) = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)
                {
                    let t = Owned::new(t);
                    if ResumeThread(*t) != u32::MAX {
                        resumed += 1;
                    }
                }
                more = Thread32Next(*snap, &mut entry).is_ok();
            }
        }
        resumed
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

    /// Drains a pipe on a thread, keeping about the last `keep` bytes (`usize::MAX`: everything).
    pub struct Drain {
        kept: Arc<Mutex<Vec<u8>>>,
        done: mpsc::Receiver<()>,
    }

    impl Drain {
        pub fn start(mut r: impl Read + Send + 'static, keep: usize) -> Drain {
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
                    if kept.len() > keep.saturating_mul(2) {
                        // an even cut keeps UTF-16 output aligned
                        let cut = (kept.len() - keep) & !1;
                        kept.drain(..cut);
                    }
                }
                let _ = tx.send(());
            });
            Drain { kept, done }
        }

        /// The tail of the output so far as text: see [`Drain::collect_bytes`].
        fn collect(&self, deadline: Instant) -> String {
            tail(&self.collect_bytes(deadline))
        }

        /// The output so far, once the pipe closes or at `deadline`, whichever is first. A
        /// program the process left running may hold the pipe open for good.
        pub fn collect_bytes(&self, deadline: Instant) -> Vec<u8> {
            let _ = self
                .done
                .recv_timeout(deadline.saturating_duration_since(Instant::now()));
            self.kept.lock().unwrap_or_else(|e| e.into_inner()).clone()
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
        // batch files (and cmd itself) run in cmd.exe too; other programs don't mind
        for exe in [
            r"C:\\tools\\build.cmd",
            "clean.BAT",
            "cmd.exe",
            r"C:\\Windows\\System32\\cmd",
        ] {
            let r = run(&format!(r#"{{"exe": ["{exe}"], "cwd": "/home/g/build"}}"#));
            assert!(check(&r, &ctx()).is_err(), "{exe}");
        }
        check(
            &run(r#"{"exe": ["C:\\tools\\build.exe"], "cwd": "/home/g/build"}"#),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn unknown_caller_distro_keeps_the_callers_user_and_directory() {
        let caller = Context {
            distro: None,
            user: Some("g".into()),
            cwd: Some("/home/g/app".into()),
        };
        let p = plan(
            &run(r#"{"wsl": ["id"], "distro": "Ubuntu"}"#),
            &caller,
            &vars(),
            None,
        )
        .unwrap();
        let Plan::Process { args, .. } = p else {
            panic!()
        };
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
                "id"
            ]
        );
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
    fn output_decoding() {
        let utf16: Vec<u8> = "There is no distribution\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode_output(&utf16), "There is no distribution\r\n");
        assert_eq!(decode_output("grüß dich".as_bytes()), "grüß dich");
        assert_eq!(decode_output(b""), "");
        assert_eq!(decode_output(b"a"), "a");
        // the tail of long UTF-16 output stays readable
        let long: Vec<u8> = "x"
            .repeat(TAIL_BYTES + 3)
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(tail(&long).chars().all(|c| c == 'x'));
        // wsl.exe's notice and the command's own UTF-8 output on one stream, either way round
        let notice = "wsl: A localhost proxy configuration was detected but not mirrored.\r\n";
        let error = "bash: ./deploy.sh: No such file or directory\n";
        let mixed = [utf16le(notice), error.as_bytes().to_vec()].concat();
        assert_eq!(decode_output(&mixed), format!("{notice}{error}"));
        let mixed = [b"ok\n".to_vec(), utf16le(notice), b"done\n".to_vec()].concat();
        assert_eq!(decode_output(&mixed), format!("ok\n{notice}done\n"));
        // not only Latin script: Windows translates wsl.exe's messages
        let russian = "Нет дистрибутива с указанным именем.\r\n";
        assert_eq!(decode_output(&utf16le(russian)), russian);
        // a cut one byte into UTF-16 loses that character only
        let cut = &utf16le("ab\r\ncd\r\n")[1..];
        assert_eq!(decode_output(cut), "b\r\ncd\r\n");
    }

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn output_decoding_with_lf_bytes_inside_characters() {
        // U+4E0A and U+040A have 0x0A as their low byte, U+4E00 has a NUL as its low byte
        for s in [
            "在此计算机上找不到\r\n",
            "上午\r\n",
            "一上\r\n",
            "上一个\r\n第二行\r\n",
            "Њет\r\n",
        ] {
            assert_eq!(decode_output(&utf16le(s)), s);
        }
    }

    #[test]
    fn output_decoding_of_mixed_streams() {
        let notice = "wsl: notice\r\n";
        let mix = |before: &[u8], after: &[u8]| {
            decode_output(&[before, &utf16le(notice), after].concat())
        };
        // whole UTF-8 lines of any length in front of it, and the odd-length text it can't be
        assert_eq!(mix(b"abc\n", b""), format!("abc\n{notice}"));
        assert_eq!(mix(b"ab\ncd\n", b""), format!("ab\ncd\n{notice}"));
        assert_eq!(mix(b"abc", b""), format!("abc{notice}"));
        assert_eq!(mix("grüß\n".as_bytes(), b"x"), format!("grüß\n{notice}x"));
        // UTF-16 without a line end, then UTF-8
        let joined = [utf16le("notice"), b"plain text\n".to_vec()].concat();
        assert_eq!(decode_output(&joined), "noticeplain text\n");
        // no line end at all, and a lone NUL: no panic, and the text stays
        assert_eq!(decode_output(&utf16le("no line end")), "no line end");
        assert_eq!(decode_output(&[0]), "");
        assert_eq!(decode_output(b"a\0"), "a");
    }

    #[test]
    fn programs_and_open_targets_take_linux_paths() {
        let target = |json: &str, c: &Context| {
            plan(&run(json), c, &[], None).map(|p| match p {
                Plan::Open { target, .. } => target,
                Plan::Process { program, .. } => program,
            })
        };
        let c = ctx();
        for (json, want) in [
            (r#"{"open": "/mnt/c/Users/g/r.pdf"}"#, r"C:\Users\g\r.pdf"),
            (
                r#"{"open": "./r.pdf"}"#,
                r"\\wsl.localhost\Ubuntu\home\g\app\r.pdf",
            ),
            (r#"{"exe": ["/mnt/c/tools/x.exe"]}"#, r"C:\tools\x.exe"),
            // for Windows to find or expand
            (
                r#"{"open": "https://x.example/a/b"}"#,
                "https://x.example/a/b",
            ),
            (r#"{"open": "%USERPROFILE%\\x"}"#, r"%USERPROFILE%\x"),
            (r#"{"open": "notes.txt"}"#, "notes.txt"),
            (r#"{"exe": ["notepad.exe"]}"#, "notepad.exe"),
            (r#"{"exe": ["C:\\x.exe"]}"#, r"C:\x.exe"),
        ] {
            assert_eq!(target(json, &c).unwrap(), want, "{json}");
        }
        // reported at show time, not on the click
        let none = Context::default();
        assert!(target(r#"{"open": "./r.pdf"}"#, &none).is_err());
        assert!(target(r#"{"exe": ["/home/g/x.exe"]}"#, &none).is_err());
        assert!(check(&run(r#"{"open": "./r.pdf"}"#), &none).is_err());
    }

    #[test]
    fn wslenv_from_the_run_is_merged() {
        let wslenv = |json: &str, current| {
            let p = plan(&run(json), &ctx(), &vars(), current).unwrap();
            let Plan::Process { env, .. } = p else {
                panic!()
            };
            let all: Vec<_> = env
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("WSLENV"))
                .collect();
            assert_eq!(all.len(), 1, "{all:?}");
            all[0].1.clone()
        };
        assert_eq!(
            wslenv(
                r#"{"wsl": "x", "env": {"WSLENV": "TOKEN/p", "TOKEN": "t", "FOO": "b"}}"#,
                Some("PATH/l")
            ),
            "PATH/l:TOKEN/p:TOASTBACK_ID/u:FOO/u"
        );
        assert_eq!(
            wslenv(r#"{"wsl": "x", "env": {"wslenv": "A"}}"#, None),
            "A:TOASTBACK_ID/u"
        );
    }

    #[test]
    fn home_cwd_is_for_wsl_only() {
        assert!(
            plan(
                &run(r#"{"pwsh": "x", "cwd": "~/proj"}"#),
                &ctx(),
                &vars(),
                None
            )
            .is_err()
        );
        assert!(
            plan(
                &run(r#"{"wsl": "x", "cwd": "~/proj"}"#),
                &ctx(),
                &vars(),
                None
            )
            .is_ok()
        );
    }

    /// The job really holds the callback's process tree: a grandchild that would write a file after
    /// 3 s dies with the 1 s timeout. (Windows only: runs cmd.exe.)
    #[cfg(windows)]
    #[test]
    fn timeout_kills_grandchildren() {
        let marker = std::env::temp_dir().join(format!("toastback-job-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let cmd = format!(
            r#"start /b cmd /c "ping -n 4 127.0.0.1 >nul & echo x > "{}"" & ping -n 30 127.0.0.1 >nul"#,
            marker.display()
        );
        let r = Run {
            cmd: Some(cmd),
            timeout: Some("1s".into()),
            ..Default::default()
        };
        let p = plan(&r, &Context::default(), &[], None).unwrap();
        let o = execute(&p, r.timeout().unwrap()).unwrap();
        assert!(o.timed_out);
        std::thread::sleep(std::time::Duration::from_secs(5));
        assert!(!marker.exists(), "a grandchild outlived the timeout");
    }

    #[test]
    fn tail_keeps_the_end() {
        let big = vec![b'a'; TAIL_BYTES + 10];
        assert_eq!(tail(&big).len(), TAIL_BYTES);
        assert_eq!(tail(b"short"), "short");
    }
}
