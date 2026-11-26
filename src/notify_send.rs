// SPDX-License-Identifier: GPL-3.0-or-later
//! `toastback notify-send`: libnotify's `notify-send` command line, shown as a toast (ADR 0017).
//!
//! The parser follows GLib's option rules, like notify-send itself: options before or after the
//! positionals, `--name=VALUE` or `--name VALUE`, `-tVALUE` or `-t VALUE`, clustered flags (`-pe`),
//! `--` ends the options, and a value is taken even when it starts with `-`. Every error prints
//! notify-send's message and exits 1. Output appears only with `-p` (the id) and for a chosen
//! action, as in notify-send. `-w` and `-A` wait with [`crate::wait`].
//!
//! `--id-fd` and `--selected-action-fd` name file descriptors of the calling Linux process, which
//! the Windows exe can't reach. The notify-send wrapper passes [`RECORDS_FLAG`] first: everything
//! for stdout then comes as `out TEXT` lines and everything for a descriptor as `fd N TEXT`, and the
//! wrapper delivers them.

use std::ffi::OsString;

use crate::spec::{
    Button, Progress, ProgressValue, Scenario, Sound, Spec, StockIcon, ToastDuration,
};

/// The libnotify version whose notify-send interface this follows, for `--version`.
pub const LIBNOTIFY_VERSION: &str = "0.8.9";
/// notify-send exits 1 on every error.
pub const EXIT_ERROR: u8 = 1;
/// Notification ids are `ns-<N>`; `N` is what `-p` prints and `-r` takes.
const ID_PREFIX: &str = "ns-";
/// `--transient`: how long the toast stays in the notification center.
const TRANSIENT_EXPIRES: &str = "10s";
/// First argument from the notify-send wrapper: print records instead of plain output.
pub const RECORDS_FLAG: &str = "--toastback-records";

const HELP: &str = "\
Usage:
  notify-send [OPTION…] <SUMMARY> [BODY] - create a notification

toastback's notify-send: libnotify's command line, shown as a Windows toast.

Options:
  -?, --help                     Show this help
  -u, --urgency=LEVEL            low (silent), normal or critical (urgent, with a Dismiss button)
  -t, --expire-time=TIME         0 or over 10000: a long banner (about 25 s), otherwise Windows'
                                 setting. With --wait: wait at most this many milliseconds
  -a, --app-name=APP_NAME        Shown as the toast's attribution line
  -i, --icon=ICON                A PNG or JPG path or file:// URI, or dialog-information,
                                 dialog-warning, dialog-error, dialog-question (other icon
                                 names, and images that can't be read, are left out)
  -n, --app-icon=ICON            The icon when there's no --icon
  -c, --category=TYPE[,TYPE...]  Accepted and ignored
  -e, --transient                Leave the notification center after about 10 seconds
  -h, --hint=TYPE:NAME:VALUE     Extra data; types: boolean, int, double, string, byte, variant.
                                 Used: urgency, image-path, suppress-sound, transient, value (a
                                 progress bar, 0 to 100), and the
                                 stack tags x-canonical-private-synchronous, x-dunst-stack-tag
                                 and synchronous (same tag: the new toast replaces the old one)
  -p, --print-id                 Print the notification ID
  -r, --replace-id=REPLACE_ID    Replace the notification with this ID
  -v, --version                  The libnotify version this follows, and toastback's
  --id-fd=FD                     Also write the ID to this file descriptor
  -w, --wait                     Wait until the notification is clicked or closed (with -t: at
                                 most that many milliseconds)
  -A, --action=[NAME=]Text...    A button; implies --wait. The chosen action's NAME (or its
                                 number, from 1) is printed. NAME default: a click on the toast's
                                 text. At most 5 buttons
  --selected-action-fd=FD        Also write the chosen action to this file descriptor
  --activation-token-fd=FD       Accepted; Windows has no activation token
";

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Help,
    Version,
    Show(Box<Request>),
}

/// A notification to show: the spec (without id and icon), and what decides those.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub spec: Spec,
    /// Icon candidates, best first: the first usable one becomes `icon`.
    pub icons: Vec<String>,
    /// From `-r` or a stack tag hint; `None`: a new number.
    pub number: Option<u32>,
    pub print_id: bool,
    /// `-w` or `-A`: wait for the answer, at most `wait_limit`.
    pub wait: bool,
    pub wait_limit: Option<std::time::Duration>,
    /// `(answer, NAME)` for each action: a button id or `__body`, and what to print for it.
    pub actions: Vec<(String, String)>,
    pub id_fd: Option<i32>,
    pub action_fd: Option<i32>,
    /// Output as records for the wrapper ([`RECORDS_FLAG`]).
    pub records: bool,
}

impl Request {
    /// What notify-send prints for an answer: the action's NAME. Nothing for a button or a click
    /// that isn't one of the actions (the Dismiss button, the text without a `default` action).
    pub fn action_name(&self, answer: &str) -> Option<&str> {
        self.actions
            .iter()
            .find(|(a, _)| a == answer)
            .map(|(_, name)| name.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Urgency {
    Low,
    Normal,
    Critical,
}

#[derive(Debug, Clone, PartialEq)]
enum Hint {
    Str(String),
    Int(i64),
    Bool(bool),
    /// `double` and `variant`: checked, never used.
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Flag,
    Text,
    Int,
}

struct Opt {
    short: Option<char>,
    long: &'static str,
    kind: Kind,
}

const fn opt(short: Option<char>, long: &'static str, kind: Kind) -> Opt {
    Opt { short, long, kind }
}

/// notify-send 0.8's options.
const OPTIONS: &[Opt] = &[
    opt(Some('?'), "help", Kind::Flag),
    opt(Some('u'), "urgency", Kind::Text),
    opt(Some('t'), "expire-time", Kind::Int),
    opt(Some('a'), "app-name", Kind::Text),
    opt(Some('i'), "icon", Kind::Text),
    opt(Some('n'), "app-icon", Kind::Text),
    opt(Some('c'), "category", Kind::Text),
    opt(Some('e'), "transient", Kind::Flag),
    opt(Some('h'), "hint", Kind::Text),
    opt(Some('p'), "print-id", Kind::Flag),
    opt(None, "id-fd", Kind::Int),
    opt(Some('r'), "replace-id", Kind::Int),
    opt(Some('w'), "wait", Kind::Flag),
    opt(Some('A'), "action", Kind::Text),
    opt(None, "selected-action-fd", Kind::Int),
    opt(None, "activation-token-fd", Kind::Int),
    opt(Some('v'), "version", Kind::Flag),
];

/// The options as given.
#[derive(Default)]
struct Raw {
    urgency: Option<Urgency>,
    app_name: Option<String>,
    icon: Option<String>,
    app_icon: Option<String>,
    transient: bool,
    hints: Vec<(String, Hint)>,
    print_id: bool,
    replace_id: Option<i64>,
    version: bool,
    expire_time: Option<i64>,
    wait: bool,
    actions: Vec<String>,
    id_fd: Option<i64>,
    action_fd: Option<i64>,
    positionals: Vec<String>,
}

impl Raw {
    /// Take one option and its value (`None` for flags). `Ok(false)`: `--help`, stop here.
    fn take(&mut self, o: &Opt, shown: &str, value: Option<String>) -> Result<bool, String> {
        let int = || -> Result<i64, String> {
            let v = value.as_deref().unwrap_or_default();
            parse_int(v).ok_or_else(|| format!("Cannot parse integer value “{v}” for {shown}"))
        };
        match o.long {
            "help" => return Ok(false),
            "urgency" => self.urgency = Some(parse_urgency(value.as_deref().unwrap_or_default())?),
            "expire-time" => self.expire_time = Some(int()?),
            "app-name" => self.app_name = value,
            "icon" => self.icon = value,
            "app-icon" => self.app_icon = value,
            "category" => {}
            "transient" => self.transient = true,
            "hint" => self
                .hints
                .push(parse_hint(value.as_deref().unwrap_or_default())?),
            "print-id" => self.print_id = true,
            "replace-id" => self.replace_id = Some(int()?),
            "version" => self.version = true,
            "wait" => self.wait = true,
            "action" => self.actions.extend(value),
            "id-fd" => self.id_fd = Some(int()?),
            "selected-action-fd" => self.action_fd = Some(int()?),
            // Windows has no activation token to hand out
            _ => {
                if o.kind == Kind::Int {
                    int()?;
                }
            }
        }
        Ok(true)
    }
}

/// Parse notify-send's arguments (without the program name).
pub fn parse(args: &[OsString]) -> Result<Parsed, String> {
    let args = args
        .iter()
        .map(|a| {
            a.to_str().map(str::to_string).ok_or_else(|| {
                format!(
                    "Invalid UTF-8 provided for parameter: {}",
                    a.to_string_lossy()
                )
            })
        })
        .collect::<Result<Vec<String>, String>>()?;
    let (records, args) = match args.split_first() {
        Some((first, rest)) if first == RECORDS_FLAG => (true, rest),
        _ => (false, args.as_slice()),
    };
    let mut raw = Raw::default();
    let mut i = 0;
    let mut options_done = false;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if options_done || arg == "-" || !arg.starts_with('-') {
            raw.positionals.push(arg.clone());
            continue;
        }
        if arg == "--" {
            options_done = true;
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let o = OPTIONS
                .iter()
                .find(|o| o.long == name)
                .ok_or_else(|| format!("Unknown option {arg}"))?;
            let shown = format!("--{name}");
            let value = match (o.kind, inline) {
                (Kind::Flag, None) => None,
                (Kind::Flag, Some(_)) => {
                    return Err(format!("Option {shown} does not take an argument"));
                }
                (_, Some(v)) => Some(v),
                (_, None) => {
                    let v = args
                        .get(i)
                        .ok_or_else(|| format!("Missing argument for {shown}"))?;
                    i += 1;
                    Some(v.clone())
                }
            };
            if !raw.take(o, &shown, value)? {
                return Ok(Parsed::Help);
            }
            continue;
        }
        // a cluster of short options: `-pe`, `-t5000`, `-pt 5000`
        let cluster = &arg[1..];
        for (at, c) in cluster.char_indices() {
            let o = OPTIONS
                .iter()
                .find(|o| o.short == Some(c))
                .ok_or_else(|| format!("Unknown option {arg}"))?;
            let shown = format!("-{c}");
            if o.kind == Kind::Flag {
                if !raw.take(o, &shown, None)? {
                    return Ok(Parsed::Help);
                }
                continue;
            }
            let rest = &cluster[at + c.len_utf8()..];
            let value = if rest.is_empty() {
                let v = args
                    .get(i)
                    .ok_or_else(|| format!("Missing argument for {shown}"))?;
                i += 1;
                v.clone()
            } else {
                rest.to_string()
            };
            raw.take(o, &shown, Some(value))?;
            break;
        }
    }
    if raw.version {
        return Ok(Parsed::Version);
    }
    let mut req = request(raw)?;
    req.records = records;
    Ok(Parsed::Show(Box::new(req)))
}

/// Map the parsed options onto a spec.
fn request(raw: Raw) -> Result<Request, String> {
    let (summary, body) = match raw.positionals.as_slice() {
        [] => return Err("No summary specified.".into()),
        [s] => (s.clone(), None),
        [s, b] => (s.clone(), Some(b.as_str())),
        _ => return Err("Invalid number of options.".into()),
    };
    let body = body
        .map(compress)
        .transpose()?
        .map(|b| strip_markup(&b))
        .filter(|b| !b.trim().is_empty());
    // a toast needs a title: an empty summary takes the body's place
    let (title, body) = match (summary.trim().is_empty(), body) {
        (false, body) => (summary, body),
        (true, Some(body)) => (body, None),
        (true, None) => return Err("No summary specified.".into()),
    };

    let mut urgency = raw.urgency.unwrap_or(Urgency::Normal);
    let mut transient = raw.transient;
    let mut silent = false;
    let mut percent = None;
    let mut image_path = None;
    let mut tag = None;
    // libnotify sends the hints after -u and -e, so a hint wins
    for (name, value) in raw.hints {
        match (name.as_str(), value) {
            ("urgency", Hint::Int(n)) => {
                urgency = match n {
                    0 => Urgency::Low,
                    2 => Urgency::Critical,
                    _ => Urgency::Normal,
                }
            }
            ("image-path" | "image_path", Hint::Str(s)) => image_path = Some(s),
            ("suppress-sound", Hint::Bool(b)) => silent = b,
            // a percentage: volume, brightness, a download
            ("value", Hint::Int(n)) => percent = Some(n.clamp(0, 100)),
            ("transient", Hint::Bool(b)) => transient = b,
            (
                "x-canonical-private-synchronous"
                | "x-dunst-stack-tag"
                | "synchronous"
                | "private-synchronous",
                Hint::Str(s),
            ) if !s.is_empty() => tag = Some(s),
            _ => {}
        }
    }

    // -A [NAME=]Text: a button, or with NAME `default`, a click on the toast's text. Unnamed
    // actions are numbered from 1 (libnotify's help says 0; its code counts from 1).
    let mut buttons = Vec::new();
    let mut actions = Vec::new();
    for (i, a) in raw.actions.iter().enumerate() {
        let (name, label) = match a.split_once('=') {
            Some((name, label)) => (name.trim().to_string(), label.to_string()),
            None => ((i + 1).to_string(), a.clone()),
        };
        if name == "default" {
            actions.push((crate::spec::BODY_ACTION.to_string(), name));
            continue;
        }
        let id = format!("a{}", buttons.len() + 1);
        let label = if label.trim().is_empty() {
            name.clone()
        } else {
            label
        };
        buttons.push(Button {
            id: id.clone(),
            label,
            ..Default::default()
        });
        actions.push((id, name));
    }
    if buttons.len() > crate::spec::MAX_BUTTONS {
        return Err(format!(
            "toastback shows at most {} actions as buttons",
            crate::spec::MAX_BUTTONS
        ));
    }
    let critical = urgency == Urgency::Critical;
    // the urgent scenario needs a button
    if critical && buttons.is_empty() {
        buttons.push(Button {
            id: "dismiss".into(),
            label: "Dismiss".into(),
            ..Default::default()
        });
    }
    let wait = raw.wait || !actions.is_empty();
    let fd = |n: Option<i64>| n.and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0);
    let spec = Spec {
        title,
        body,
        attribution: raw.app_name.filter(|a| !a.trim().is_empty()),
        scenario: if critical {
            Scenario::Urgent
        } else {
            Scenario::Default
        },
        sound: if silent || urgency == Urgency::Low {
            Sound::Silent
        } else {
            Sound::Default
        },
        buttons,
        expires: transient.then(|| TRANSIENT_EXPIRES.into()),
        // 0 is libnotify's "never expires": the longest banner Windows has
        duration: match raw.expire_time {
            Some(t) if t == 0 || t > 10_000 => ToastDuration::Long,
            _ => ToastDuration::Short,
        },
        progress: percent.map(|n| Progress {
            value: ProgressValue::Fraction(n as f64 / 100.0),
            status: None,
            title: None,
            label: None,
        }),
        ..Default::default()
    };
    // the image-path hint is the notification's image; --icon and --app-icon come after it
    let icons = [image_path, raw.icon, raw.app_icon]
        .into_iter()
        .flatten()
        .filter_map(|s| icon_source(&s))
        .collect();
    let number = match raw.replace_id {
        Some(n) if n > 0 => Some(u32::try_from(n).unwrap_or(u32::MAX)),
        _ => tag.as_deref().map(tag_number),
    };
    Ok(Request {
        spec,
        icons,
        number,
        print_id: raw.print_id,
        wait,
        // libnotify waits forever with 0 (never) and the server's default (-1)
        wait_limit: raw
            .expire_time
            .filter(|t| wait && *t > 0)
            .map(|t| std::time::Duration::from_millis(t.unsigned_abs())),
        actions,
        id_fd: fd(raw.id_fd),
        action_fd: fd(raw.action_fd),
        records: false,
    })
}

/// Prints for notify-send: plain, or as records for the wrapper.
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows `run` prints
struct Output {
    records: bool,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Output {
    /// The lines to print for `text`, sent to stdout (`to_stdout`) and/or descriptor `fd`.
    fn lines(&self, text: &str, to_stdout: bool, fd: Option<i32>) -> Vec<String> {
        // a record is one line
        let text = text.replace(['\n', '\r'], " ");
        let mut out = Vec::new();
        if to_stdout {
            out.push(if self.records {
                format!("out {text}")
            } else {
                text.clone()
            });
        }
        if let (true, Some(fd)) = (self.records, fd) {
            out.push(format!("fd {fd} {text}"));
        }
        out
    }
}

/// The id of notification number `n`.
pub fn id_for(n: u32) -> String {
    format!("{ID_PREFIX}{n}")
}

/// GLib's integer syntax: decimal, or hex with `0x`; an optional sign.
fn parse_int(s: &str) -> Option<i64> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let n = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) => i64::from_str_radix(hex, 16).ok()?,
        None => digits.parse::<i64>().ok()?,
    };
    let n = if neg { -n } else { n };
    i32::try_from(n).ok().map(i64::from)
}

fn parse_urgency(s: &str) -> Result<Urgency, String> {
    match s.to_ascii_lowercase().as_str() {
        "low" => Ok(Urgency::Low),
        "normal" => Ok(Urgency::Normal),
        "critical" => Ok(Urgency::Critical),
        _ => Err(format!(
            "Unknown urgency {s} specified. Known urgency levels: low, normal, critical."
        )),
    }
}

/// `TYPE:NAME:VALUE`, checked like libnotify checks it.
fn parse_hint(s: &str) -> Result<(String, Hint), String> {
    let mut parts = s.splitn(3, ':');
    let (Some(kind), Some(name), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
        return Err("Invalid hint syntax specified. Use TYPE:NAME:VALUE.".into());
    };
    let bad = || format!("Invalid hint value “{value}” for type {kind}");
    let starts_with_digit = value.starts_with(|c: char| c.is_ascii_digit());
    let hint = match kind.to_ascii_lowercase().as_str() {
        "string" => Hint::Str(value.to_string()),
        "int" if starts_with_digit => Hint::Int(value.parse().map_err(|_| bad())?),
        "double" if starts_with_digit => {
            value.parse::<f64>().map_err(|_| bad())?;
            Hint::Other
        }
        "byte" => {
            let n = match value.strip_prefix("0x") {
                Some(hex) => u8::from_str_radix(hex, 16),
                None => value.parse::<u8>(),
            };
            Hint::Int(i64::from(n.map_err(|_| bad())?))
        }
        "boolean" => Hint::Bool(
            value.eq_ignore_ascii_case("true")
                || value.starts_with(|c: char| c.is_ascii_digit() && c != '0'),
        ),
        "variant" => Hint::Other,
        "int" | "double" => return Err(bad()),
        _ => {
            return Err(format!(
                "Invalid hint type “{kind}”. Valid types are boolean, int, double, string, byte and variant."
            ));
        }
    };
    Ok((name.to_string(), hint))
}

/// What notify-send does to the body (GLib's `g_strcompress`): `\n`, `\t`, `\b`, `\f`, `\r`,
/// `\v`, octal `\NNN`; any other escaped character stands for itself.
fn compress(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&c) = b.get(i) else { break }; // a trailing backslash is dropped
        if (b'0'..=b'7').contains(&c) {
            let mut v: u32 = 0;
            let start = i;
            while i < b.len() && i - start < 3 && (b'0'..=b'7').contains(&b[i]) {
                v = v * 8 + u32::from(b[i] - b'0');
                i += 1;
            }
            out.push(v as u8); // GLib truncates the same way
            continue;
        }
        out.push(match c {
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            other => other,
        });
        i += 1;
    }
    String::from_utf8(out).map_err(|_| "Invalid UTF-8 provided for parameter: body".into())
}

/// Tags of the notification body markup (freedesktop's, plus the Pango ones some daemons accept).
const MARKUP_TAGS: &[&str] = &[
    "a", "b", "big", "i", "img", "s", "small", "span", "sub", "sup", "tt", "u",
];

/// The body as plain text: markup tags removed, entities decoded. A `<` that doesn't start one
/// of the known tags stays.
fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        match tail.find('>') {
            Some(end) if is_markup_tag(&tail[1..end]) => rest = &tail[end + 1..],
            _ => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    decode_entities(&out)
}

fn is_markup_tag(inner: &str) -> bool {
    let inner = inner.strip_prefix('/').unwrap_or(inner);
    let len = inner
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(inner.len());
    let (name, after) = inner.split_at(len);
    MARKUP_TAGS.contains(&name.to_ascii_lowercase().as_str())
        && (after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == '/'))
}

/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&#N;` and `&#xN;`; anything else stays.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let decoded = tail.find(';').filter(|&e| e <= 10).and_then(|end| {
            let c = match &tail[1..end] {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                n => n
                    .strip_prefix("#x")
                    .or_else(|| n.strip_prefix("#X"))
                    .map(|h| u32::from_str_radix(h, 16))
                    .or_else(|| n.strip_prefix('#').map(str::parse::<u32>))
                    .and_then(Result::ok)
                    .and_then(char::from_u32),
            }?;
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// What an `--icon`, `--app-icon` or `image-path` value names: an image path (from a path or a
/// `file://` URI), a built-in icon, or nothing (other icon names come from Linux icon themes).
fn icon_source(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(path) = s.strip_prefix("file://") {
        // file:///path, or file://localhost/path
        let path = path.strip_prefix("localhost").unwrap_or(path);
        return Some(percent_decode(path)).filter(|p| !p.is_empty());
    }
    if s.contains('/') || s.contains('\\') {
        return Some(s.to_string());
    }
    let name = s.strip_suffix("-symbolic").unwrap_or(s);
    let stock = match name {
        "dialog-information" | "dialog-info" | "info" | "information" => StockIcon::Info,
        "dialog-warning" | "warning" | "emblem-warning" => StockIcon::Warning,
        "dialog-error" | "error" | "emblem-error" => StockIcon::Error,
        "dialog-question" | "question" => StockIcon::Question,
        _ => return None,
    };
    Some(stock.name().to_string())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Images Windows can't show in a toast, whatever the file holds.
fn is_unsupported_image(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    matches!(ext.as_deref(), Some("svg" | "svgz" | "xpm"))
}

/// The first candidate `usable` accepts; built-in icons always are.
fn choose_icon(candidates: &[String], usable: impl Fn(&str) -> bool) -> Option<String> {
    candidates
        .iter()
        .find(|c| {
            StockIcon::from_name(c).is_some() || (!is_unsupported_image(c) && usable(c.as_str()))
        })
        .cloned()
}

/// A stack tag's number: the same tag gives the same id, so the new toast replaces the old one.
fn tag_number(tag: &str) -> u32 {
    // FNV-1a: stable across versions and platforms
    let h = tag.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    h % 0x7fff_ffff + 1
}

/// Does `new` differ from the shown `old` only in its progress bar's value? Then an update shows it.
fn only_progress_changed(old: &Spec, new: &Spec) -> bool {
    let shape = |s: &Spec| {
        s.progress.as_ref().map(|p| Progress {
            value: ProgressValue::Fraction(0.0),
            ..p.clone()
        })
    };
    let strip = |s: &Spec| Spec {
        progress: None,
        // where the caller ran doesn't show
        context: Default::default(),
        ..s.clone()
    };
    new.progress.is_some() && shape(old) == shape(new) && strip(old) == strip(new)
}

/// Update the toast `spec` would replace, if that's all it takes. `Some(id)`: done.
#[cfg(windows)]
fn update_in_place(store: &crate::store::Store, spec: &Spec) -> Option<String> {
    use crate::spec::Mode;
    use crate::toast;

    let id = spec.id.clone()?;
    let old = store.load_pending(&id).ok().flatten()?;
    if old.done || old.spec.mode != Mode::Toast || !only_progress_changed(&old.spec, spec) {
        return None;
    }
    let progress = spec.progress.as_ref()?;
    if !toast::update(&id, &toast::progress_data(progress)).ok()? {
        return None;
    }
    let _ = store.set_progress(&id, &old.token, progress);
    let _ = store.log(
        "updated",
        serde_json::json!({ "id": id, "action": progress.value.to_xml_value() }),
    );
    Some(id)
}

/// Show what notify-send's command line asks for. Returns the exit code.
#[cfg(windows)]
pub fn run(
    store: &crate::store::Store,
    args: &[OsString],
    ctx: &crate::spec::Context,
    caller: Option<&crate::wait::Caller>,
) -> u8 {
    use crate::wait::Outcome;
    use crate::{install, show, toast, wait};

    let req = match parse(args) {
        Ok(Parsed::Help) => {
            out!("{HELP}");
            return 0;
        }
        Ok(Parsed::Version) => {
            outln!(
                "notify-send {LIBNOTIFY_VERSION} (toastback {})",
                env!("CARGO_PKG_VERSION")
            );
            return 0;
        }
        Ok(Parsed::Show(r)) => *r,
        Err(msg) => {
            errln!("{msg}");
            return EXIT_ERROR;
        }
    };
    if !req.records && (req.id_fd.is_some() || req.action_fd.is_some()) {
        errln!(
            "notify-send: --id-fd and --selected-action-fd work through toastback's notify-send wrapper only"
        );
        return EXIT_ERROR;
    }
    let output = Output {
        records: req.records,
    };
    let print = |text: &str, to_stdout: bool, fd: Option<i32>| {
        for line in output.lines(text, to_stdout, fd) {
            outln!("{line}");
        }
    };
    let mut spec = req.spec.clone();
    let number = req.number;
    spec.context.fill_from(ctx);
    // notify-send never fails over an icon: one that can't be read is left out
    let icon = choose_icon(&req.icons, |p| {
        crate::context::to_windows_path(p, &spec.context)
            .is_ok_and(|w| std::fs::metadata(w).is_ok_and(|m| m.is_file()))
    });
    spec.icon = icon;
    let number = match number {
        Some(n) => n,
        None => match getrandom::u32() {
            Ok(r) => r % 0x7fff_ffff + 1,
            Err(e) => {
                errln!("Failed to show notification: OS RNG: {e}");
                return EXIT_ERROR;
            }
        },
    };
    spec.id = Some(id_for(number));
    if let Err(e) = spec.validate() {
        errln!("notify-send: {e:#}");
        return EXIT_ERROR;
    }
    match install::is_installed(store) {
        Ok(true) => {}
        Ok(false) => {
            errln!("notify-send: toastback isn't installed; run `toastback.exe install` first");
            return EXIT_ERROR;
        }
        Err(e) => {
            errln!("notify-send: {e:#}; run `toastback.exe install` to repair it");
            return EXIT_ERROR;
        }
    }
    // a new value for a progress toast that's still shown: move its bar in place, as the daemons
    // update an OSD; replacing it would close its banner without a new one (ADR 0019)
    let id = match update_in_place(store, &spec) {
        Some(id) => id,
        None => match show::show_spec(store, spec) {
            Ok(id) => id,
            Err((_, e)) => {
                errln!("Failed to show notification: {e:#}");
                return EXIT_ERROR;
            }
        },
    };
    print(&number.to_string(), req.print_id, req.id_fd);
    if !req.wait {
        return 0;
    }
    // notify-send exits 0 however the wait ends
    match wait::wait(store, &id, req.wait_limit, caller) {
        Some(Outcome::Answer(a)) => {
            if let Some(name) = req.action_name(&a) {
                print(name, true, req.action_fd);
            }
        }
        Some(Outcome::TimedOut) => errln!("Wait timeout expired"),
        Some(Outcome::Interrupted) => {
            // as libnotify does on SIGINT
            errln!("Wait cancelled, closing notification");
            let _ = toast::remove(&id);
            let _ = store.remove_pending(&id);
        }
        Some(Outcome::Closed(_)) | None => {}
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Parsed, String> {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        parse(&args)
    }

    fn req(args: &[&str]) -> Request {
        match parse_args(args) {
            Ok(Parsed::Show(r)) => *r,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    fn err(args: &[&str]) -> String {
        match parse_args(args) {
            Err(e) => e,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    #[test]
    fn summary_and_body() {
        let r = req(&["Backup", "42 GB"]);
        assert_eq!(r.spec.title, "Backup");
        assert_eq!(r.spec.body.as_deref(), Some("42 GB"));
        assert_eq!(r.number, None);
        assert!(!r.print_id);
        assert_eq!(r.spec.scenario, Scenario::Default);
        assert!(r.spec.buttons.is_empty());
        r.spec.validate().unwrap();

        assert_eq!(req(&["only"]).spec.body, None);
        assert_eq!(err(&[]), "No summary specified.");
        assert_eq!(err(&["a", "b", "c"]), "Invalid number of options.");
        // an empty summary: the body becomes the title
        let r = req(&["", "text"]);
        assert_eq!((r.spec.title.as_str(), r.spec.body), ("text", None));
        assert_eq!(err(&[""]), "No summary specified.");
    }

    #[test]
    fn options_follow_glib_rules() {
        // options after the positionals, = and separate values, attached short values, clusters
        let r = req(&["T", "-u", "low", "--app-name=CI", "-pe", "-r42"]);
        assert_eq!(r.spec.sound, Sound::Silent);
        assert_eq!(r.spec.attribution.as_deref(), Some("CI"));
        assert!(r.print_id);
        assert_eq!(r.spec.expires.as_deref(), Some(TRANSIENT_EXPIRES));
        assert_eq!(r.number, Some(42));
        let r = req(&["-pr", "7", "T"]);
        assert_eq!((r.print_id, r.number), (true, Some(7)));
        // a value is taken even when it starts with -
        let r = req(&["-a", "-x-", "T"]);
        assert_eq!(r.spec.attribution.as_deref(), Some("-x-"));
        // -- ends the options; - is a positional
        let r = req(&["--", "-T", "-u"]);
        assert_eq!(
            (r.spec.title.as_str(), r.spec.body.as_deref()),
            ("-T", Some("-u"))
        );
        assert_eq!(req(&["-"]).spec.title, "-");
        // -t is checked, not used; -c is ignored
        req(&["-t", "5000", "-c", "device,email", "T"]);
        req(&["--expire-time=-1", "T"]);
        assert_eq!(
            err(&["-t", "soon", "T"]),
            "Cannot parse integer value “soon” for -t"
        );
        assert_eq!(err(&["-x", "T"]), "Unknown option -x");
        assert_eq!(err(&["--bogus", "T"]), "Unknown option --bogus");
        assert_eq!(err(&["T", "-u"]), "Missing argument for -u");
        assert_eq!(
            err(&["--wait=1", "T"]),
            "Option --wait does not take an argument"
        );
        assert_eq!(
            err(&["-u", "HIGH", "T"]),
            "Unknown urgency HIGH specified. Known urgency levels: low, normal, critical."
        );
        // a negative number in the text is an option, as in notify-send
        assert_eq!(err(&["Temp", "-5 degrees"]), "Unknown option -5 degrees");
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse_args(&["-?"]), Ok(Parsed::Help));
        assert_eq!(parse_args(&["--help", "-x"]), Ok(Parsed::Help));
        assert_eq!(parse_args(&["-v"]), Ok(Parsed::Version));
        assert_eq!(
            parse_args(&["--version", "a", "b", "c"]),
            Ok(Parsed::Version)
        );
        // -h is --hint, as in notify-send
        assert_eq!(err(&["-h"]), "Missing argument for -h");
    }

    #[test]
    fn actions_and_wait() {
        let r = req(&["-A", "yes=Yes", "-A", "No", "-A", "default=Open", "Q"]);
        assert!(r.wait);
        assert_eq!(r.wait_limit, None);
        let labels: Vec<_> = r
            .spec
            .buttons
            .iter()
            .map(|b| (b.id.as_str(), b.label.as_str()))
            .collect();
        assert_eq!(labels, [("a1", "Yes"), ("a2", "No")]);
        // unnamed actions are numbered from 1, counting every action
        assert_eq!(r.action_name("a1"), Some("yes"));
        assert_eq!(r.action_name("a2"), Some("2"));
        assert_eq!(r.action_name("__body"), Some("default"));
        assert_eq!(r.action_name("dismiss"), None);
        r.spec.validate().unwrap();
        // no `default` action: a click on the text prints nothing
        assert_eq!(req(&["-A", "ok=OK", "Q"]).action_name("__body"), None);
        // an empty label shows the name
        assert_eq!(req(&["-A", "later=", "Q"]).spec.buttons[0].label, "later");

        // -t limits a wait only
        let limit = |args: &[&str]| req(args).wait_limit;
        assert_eq!(
            limit(&["-w", "-t", "5000", "T"]),
            Some(std::time::Duration::from_secs(5))
        );
        assert_eq!(limit(&["-t", "5000", "T"]), None);
        assert_eq!(limit(&["-w", "-t", "0", "T"]), None);
        assert_eq!(limit(&["-w", "-t", "-1", "T"]), None);
        let r = req(&["-w", "T"]);
        assert!(r.wait && r.actions.is_empty());
        assert!(!req(&["T"]).wait);

        // critical gets Dismiss only when there are no buttons
        let r = req(&["-u", "critical", "-A", "fix=Fix it", "Disk full"]);
        assert_eq!(r.spec.buttons.len(), 1);
        assert_eq!(r.spec.buttons[0].label, "Fix it");
        let r = req(&["-u", "critical", "-A", "default=Open", "Disk full"]);
        assert_eq!(r.spec.buttons[0].label, "Dismiss");
        r.spec.validate().unwrap();

        let six: Vec<String> = (1..=6).map(|i| format!("-A=b{i}")).collect();
        let mut args: Vec<&str> = six.iter().map(String::as_str).collect();
        args.push("T");
        assert!(err(&args).contains("at most 5"));
    }

    #[test]
    fn progress_and_duration() {
        let r = req(&[
            "-h",
            "int:value:40",
            "-h",
            "string:x-dunst-stack-tag:vol",
            "Volume",
        ]);
        assert_eq!(
            r.spec.progress.as_ref().map(|p| p.value),
            Some(ProgressValue::Fraction(0.4))
        );
        r.spec.validate().unwrap();
        // clamped, like the daemons do
        let value = |v: &str| req(&["-h", v, "T"]).spec.progress.unwrap().value;
        assert_eq!(value("int:value:150"), ProgressValue::Fraction(1.0));
        assert_eq!(req(&["T"]).spec.progress, None);

        let duration = |args: &[&str]| req(args).spec.duration;
        assert_eq!(duration(&["T"]), ToastDuration::Short);
        assert_eq!(duration(&["-t", "5000", "T"]), ToastDuration::Short);
        assert_eq!(duration(&["-t", "10001", "T"]), ToastDuration::Long);
        assert_eq!(duration(&["-t", "0", "T"]), ToastDuration::Long);
        assert_eq!(duration(&["-t", "-1", "T"]), ToastDuration::Short);
    }

    #[test]
    fn only_a_new_value_updates_in_place() {
        let spec = |args: &[&str]| req(args).spec;
        let at40 = spec(&[
            "-h",
            "int:value:40",
            "-h",
            "string:synchronous:vol",
            "Volume",
        ]);
        let at70 = spec(&[
            "-h",
            "int:value:70",
            "-h",
            "string:synchronous:vol",
            "Volume",
        ]);
        assert!(only_progress_changed(&at40, &at70));
        let mut elsewhere = at70.clone();
        elsewhere.context.cwd = Some("/tmp".into());
        assert!(only_progress_changed(&at40, &elsewhere));
        // anything else that shows: a new toast
        for other in [
            spec(&["-h", "int:value:70", "Volume", "muted"]),
            spec(&["-h", "int:value:70", "Brightness"]),
            spec(&["-h", "int:value:70", "-u", "critical", "Volume"]),
            // `run` picks the icon before comparing
            Spec {
                icon: Some("warning".into()),
                ..at70.clone()
            },
            spec(&["Volume"]),
        ] {
            assert!(!only_progress_changed(&at40, &other), "{other:?}");
        }
        // a toast without a bar gets one by being shown again
        assert!(!only_progress_changed(&spec(&["Volume"]), &at70));
    }

    #[test]
    fn descriptors_and_records() {
        let r = req(&[
            RECORDS_FLAG,
            "-p",
            "--id-fd=3",
            "--selected-action-fd",
            "4",
            "T",
        ]);
        assert!(r.records && r.print_id);
        assert_eq!((r.id_fd, r.action_fd), (Some(3), Some(4)));
        // only as the first argument
        assert_eq!(
            err(&["T", RECORDS_FLAG]),
            format!("Unknown option {RECORDS_FLAG}")
        );
        assert!(!req(&["--id-fd", "3", "T"]).records);
        req(&["--activation-token-fd", "5", "T"]);
        assert!(err(&["--id-fd", "x", "T"]).contains("Cannot parse integer"));

        let plain = Output { records: false };
        assert_eq!(plain.lines("42", true, Some(3)), ["42"]);
        assert!(plain.lines("42", false, Some(3)).is_empty());
        let records = Output { records: true };
        assert_eq!(records.lines("42", true, Some(3)), ["out 42", "fd 3 42"]);
        assert_eq!(records.lines("a\nb", false, Some(4)), ["fd 4 a b"]);
        assert_eq!(records.lines("x", true, None), ["out x"]);
    }

    #[test]
    fn critical_is_urgent_with_a_dismiss_button() {
        let r = req(&["-u", "CRITICAL", "Disk full"]);
        assert_eq!(r.spec.scenario, Scenario::Urgent);
        assert_eq!(r.spec.sound, Sound::Default);
        assert_eq!(r.spec.buttons.len(), 1);
        assert_eq!(r.spec.buttons[0].label, "Dismiss");
        assert!(r.spec.buttons[0].run.is_none());
        r.spec.validate().unwrap();
    }

    #[test]
    fn hints() {
        let r = req(&[
            "-h",
            "STRING:image-path:/usr/share/pixmaps/x.png",
            "-h",
            "boolean:suppress-sound:true",
            "-h",
            "boolean:transient:1",
            "-h",
            "string:x-dunst-stack-tag:volume",
            "-h",
            "int:value:40",
            "-h",
            "double:ratio:0.5",
            "-h",
            "variant:x:<1>",
            "-h",
            "string:sound-name:message-new-instant",
            "-i",
            "dialog-warning",
            "T",
        ]);
        assert_eq!(r.icons, ["/usr/share/pixmaps/x.png", "warning"]);
        assert_eq!(r.spec.sound, Sound::Silent);
        assert_eq!(r.spec.expires.as_deref(), Some(TRANSIENT_EXPIRES));
        // the same tag, the same number; -r wins over it
        assert_eq!(r.number, Some(tag_number("volume")));
        assert_eq!(
            req(&["-h", "string:synchronous:volume", "T"]).number,
            r.number
        );
        assert_ne!(
            req(&["-h", "string:synchronous:brightness", "T"]).number,
            r.number
        );
        assert_eq!(
            req(&["-h", "string:synchronous:volume", "-r", "9", "T"]).number,
            Some(9)
        );
        // a value may contain colons
        let r = req(&["-h", "string:image-path:C:\\icons\\a.png", "T"]);
        assert_eq!(r.icons, ["C:\\icons\\a.png"]);
        // the urgency hint, sent after -u, wins
        let r = req(&["-u", "low", "-h", "byte:urgency:2", "T"]);
        assert_eq!(r.spec.scenario, Scenario::Urgent);
        let r = req(&["-h", "byte:urgency:0x00", "T"]);
        assert_eq!(r.spec.sound, Sound::Silent);

        assert_eq!(
            err(&["-h", "string:only-two", "T"]),
            "Invalid hint syntax specified. Use TYPE:NAME:VALUE."
        );
        assert!(err(&["-h", "int:value:-1", "T"]).contains("Invalid hint value"));
        assert!(err(&["-h", "byte:urgency:256", "T"]).contains("Invalid hint value"));
        assert!(err(&["-h", "float:x:1", "T"]).contains("Invalid hint type"));
    }

    #[test]
    fn icons() {
        assert_eq!(
            icon_source("dialog-error-symbolic").as_deref(),
            Some("error")
        );
        assert_eq!(icon_source("dialog-information").as_deref(), Some("info"));
        assert_eq!(icon_source("firefox"), None);
        assert_eq!(icon_source("icon.png"), None); // a name, as for the daemons
        assert_eq!(icon_source("./icon.png").as_deref(), Some("./icon.png"));
        assert_eq!(
            icon_source("file:///home/g/My%20Icons/a.png").as_deref(),
            Some("/home/g/My Icons/a.png")
        );
        assert_eq!(
            icon_source("file://localhost/tmp/a.png").as_deref(),
            Some("/tmp/a.png")
        );
        // -i before -n; the first usable candidate wins, built-ins always are
        let r = req(&["-n", "/apps/app.png", "-i", "/missing.png", "T"]);
        assert_eq!(r.icons, ["/missing.png", "/apps/app.png"]);
        assert_eq!(
            choose_icon(&r.icons, |p| p != "/missing.png").as_deref(),
            Some("/apps/app.png")
        );
        assert_eq!(choose_icon(&r.icons, |_| false), None);
        assert_eq!(
            choose_icon(&["/a.svg".into(), "info".into()], |_| true).as_deref(),
            Some("info")
        );
    }

    #[test]
    fn body_escapes_and_markup() {
        let body = |b: &str| req(&["T", b]).spec.body;
        assert_eq!(
            body(r"line 1\nline 2\ttab \\ \q \101").as_deref(),
            Some("line 1\nline 2\ttab \\ q A")
        );
        // the summary is used as given
        assert_eq!(req(&[r"a\nb"]).spec.title, r"a\nb");
        assert_eq!(
            body(r#"<b>Done</b> in <i>4 min</i>: <a href="https://x">log</a>"#).as_deref(),
            Some("Done in 4 min: log")
        );
        assert_eq!(
            body("<span foreground='red'>red</span><br>").as_deref(),
            Some("red<br>")
        );
        assert_eq!(
            body("a < b &amp;&amp; c > d, AT&T &#65;&#x42; &bogus;").as_deref(),
            Some("a < b && c > d, AT&T AB &bogus;")
        );
        assert_eq!(body("&lt;b&gt;").as_deref(), Some("<b>"));
        // only markup: no body
        assert_eq!(body("<b></b>"), None);
        assert_eq!(
            err(&["T", r"\377"]),
            "Invalid UTF-8 provided for parameter: body"
        );
    }

    #[test]
    fn ids() {
        assert_eq!(id_for(42), "ns-42");
        let n = tag_number("volume");
        assert!((1..=0x7fff_ffff).contains(&n));
        assert_eq!(n, tag_number("volume"));
        // -r 0 asks for a new notification
        assert_eq!(req(&["-r", "0", "T"]).number, None);
        let mut spec = req(&["T"]).spec;
        spec.id = Some(id_for(u32::MAX));
        spec.validate().unwrap();
    }
}
