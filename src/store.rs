// SPDX-License-Identifier: GPL-3.0-or-later
//! State in `%LOCALAPPDATA%\toastback`: pending notifications, the event log, the install record.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::spec::{Spec, is_safe_id};

const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;
/// How long past `expires` a choice in a dialog that opened in time still counts.
const DIALOG_GRACE_SECS: u64 = 86400;
/// Locks are held for a few file operations; waiting longer than this means something is wrong.
const LOCK_WAIT: Duration = Duration::from_secs(10);
/// The kinds of cached images: `cache\<id>-icon.<ext>` and `cache\<id>-image.<ext>`.
pub const ICON: &str = "icon";
pub const IMAGE: &str = "image";

pub struct Store {
    root: PathBuf,
}

/// A shown notification waiting for clicks: `pending\<id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pending {
    pub spec: Spec,
    pub token: String,
    pub created: u64,
    pub expires_at: u64,
    #[serde(default)]
    pub done: bool,
    /// Cached copies of `icon`/`image`, as Windows paths.
    #[serde(default)]
    pub icon_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
    /// A dialog for it opened before it expired and is still open: a choice in that dialog counts
    /// up to a day past expiry, and `prune` keeps the file that long.
    #[serde(default)]
    pub dialog_open: bool,
}

/// The fields `prune` needs, read leniently: a file written by a newer version still parses.
#[derive(Deserialize)]
struct PendingHead {
    expires_at: u64,
    #[serde(default)]
    dialog_open: bool,
}

/// What [`Store::claim`] found.
#[derive(Debug)]
pub enum Claim {
    /// This caller handles the click; the record is now marked done.
    Claimed,
    /// Removed, pruned or never shown.
    Gone,
    /// Past `expires` (and no dialog that opened in time).
    Expired,
    /// Shown again under the same id since (a new token).
    Replaced,
    /// Another click got there first.
    Done,
}

/// `install.json`: what `install` changed, so `uninstall` and `doctor` can find it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallRecord {
    pub version: String,
    pub installed_at: String,
    pub exe: String,
    /// Distros (and users) that received a shim.
    #[serde(default)]
    pub shims: Vec<ShimTarget>,
    /// `install --wsl none` was chosen, so a later install doesn't write shims on its own.
    #[serde(default)]
    pub no_shims: Option<bool>,
    #[serde(default)]
    pub path_added: bool,
}

impl InstallRecord {
    /// Did the user choose no shims? A record from before `no_shims` can't tell that from a first
    /// install that couldn't write one: an empty list there keeps meaning "none".
    pub fn shims_declined(&self) -> bool {
        self.no_shims.unwrap_or(self.shims.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShimTarget {
    pub distro: String,
    /// `None`: the distro's default user.
    pub user: Option<String>,
}

impl Store {
    /// The per-user state dir, from `%LOCALAPPDATA%`.
    pub fn open() -> Result<Store> {
        let base = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
        Ok(Store::at(PathBuf::from(base).join("toastback")))
    }

    pub fn at(root: PathBuf) -> Store {
        Store { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn installed_exe(&self) -> PathBuf {
        self.bin_dir().join("toastback.exe")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    fn pending_dir(&self) -> PathBuf {
        self.root.join("pending")
    }

    fn pending_file(&self, id: &str) -> PathBuf {
        self.pending_dir().join(format!("{id}.json"))
    }

    fn log_file(&self) -> PathBuf {
        self.root.join("log").join("toastback.jsonl")
    }

    fn install_file(&self) -> PathBuf {
        self.root.join("install.json")
    }

    /// Write a pending notification (replacing one with the same id).
    pub fn save_pending(&self, p: &Pending) -> Result<()> {
        let _lock = self.lock_pending()?;
        self.write_pending(p)
    }

    fn write_pending(&self, p: &Pending) -> Result<()> {
        let id = p.spec.id.as_deref().context("pending spec without id")?;
        write_atomic(&self.pending_file(id), &serde_json::to_vec_pretty(p)?)
    }

    /// `None` when there is no such pending notification (expired, pruned or never shown).
    pub fn load_pending(&self, id: &str) -> Result<Option<Pending>> {
        if !is_safe_id(id) {
            return Ok(None);
        }
        match fs::read(self.pending_file(id)) {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes).context("corrupt pending file")?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("read pending file"),
        }
    }

    /// Is there a pending file for `id`, readable or not (one from a newer version may not be)?
    pub fn has_pending(&self, id: &str) -> bool {
        is_safe_id(id) && self.pending_file(id).exists()
    }

    pub fn remove_pending(&self, id: &str) -> Result<()> {
        self.remove_pendings(&[id])
    }

    /// Delete these pending notifications and their cached images: one lock, one pass over the
    /// cache. A file that can't be deleted doesn't stop the others; the first error is returned.
    pub fn remove_pendings(&self, ids: &[&str]) -> Result<()> {
        let ids: Vec<&str> = ids.iter().copied().filter(|id| is_safe_id(id)).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let _lock = self.lock_pending()?;
        let mut removed = Vec::new();
        let mut failed = None;
        for id in ids {
            match remove_if_exists(&self.pending_file(id)) {
                Ok(()) => removed.push(id),
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        let (_, cache_error) = self.remove_cached(&removed);
        failed.or(cache_error).map_or(Ok(()), Err)
    }

    /// Take the click for the notification shown with `token`: mark it done, atomically, so two
    /// clicks (or a click and a dialog) never both run its callback. `from_dialog`: the choice
    /// was made in a dialog, which may count past `expires` (see [`Store::set_dialog_open`]); a
    /// late toast click or a replayed URI doesn't.
    pub fn claim(&self, id: &str, token: &str, now: u64, from_dialog: bool) -> Result<Claim> {
        let _lock = self.lock_pending()?;
        let Some(mut p) = self.load_pending(id)? else {
            return Ok(Claim::Gone);
        };
        if !token_matches(&p.token, token) {
            return Ok(Claim::Replaced);
        }
        if p.done {
            return Ok(Claim::Done);
        }
        if now > deadline(p.expires_at, p.dialog_open && from_dialog) {
            return Ok(Claim::Expired);
        }
        p.done = true;
        p.dialog_open = false;
        // the caller has its copy; what's left only has to recognize a second click
        p.spec.forget_env();
        self.write_pending(&p)?;
        Ok(Claim::Claimed)
    }

    /// Record that a dialog for the notification shown with `token` opened before it expired.
    pub fn set_dialog_open(&self, id: &str, token: &str, now: u64) -> Result<()> {
        let _lock = self.lock_pending()?;
        if let Some(mut p) = self.load_pending(id)?
            && token_matches(&p.token, token)
            && !p.done
            && now <= p.expires_at
        {
            p.dialog_open = true;
            self.write_pending(&p)?;
        }
        Ok(())
    }

    /// Ids of all pending notifications.
    pub fn pending_ids(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(self.pending_dir()) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).context("read pending dir"),
        };
        let mut ids = Vec::new();
        for entry in entries {
            if let Some(id) = entry?
                .file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
            {
                ids.push(id.to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Delete pending files (and their cached images) whose notification expired. Returns the ids.
    /// One with an open dialog is kept a day longer; an unreadable one is deleted. While another
    /// process holds the pending lock, nothing is pruned: the next run does it.
    pub fn prune(&self, now: u64) -> Result<Vec<String>> {
        // scan without the lock (every toastback run prunes), then lock to re-check and delete
        let candidates: Vec<String> = self
            .pending_ids()?
            .into_iter()
            .filter(|id| self.expired(id, now))
            .collect();
        if candidates.is_empty() {
            return Ok(candidates);
        }
        // pruning can wait for the next run: it never holds up a show or a click
        let Some(_lock) = lock_file(&self.pending_lock(), Duration::ZERO)? else {
            return Ok(Vec::new());
        };
        let expired: Vec<&str> = candidates
            .iter()
            .map(String::as_str)
            .filter(|id| self.expired(id, now))
            .collect();
        // Images first: the pending file is what brings prune back, so one with an image that
        // can't be deleted now (in use) stays until a later run gets it.
        let (stuck, _) = self.remove_cached(&expired);
        Ok(expired
            .into_iter()
            .filter(|id| !stuck.contains(id) && remove_if_exists(&self.pending_file(id)).is_ok())
            .map(str::to_string)
            .collect())
    }

    fn expired(&self, id: &str, now: u64) -> bool {
        let head = fs::read(self.pending_file(id));
        match head {
            Ok(b) => match serde_json::from_slice::<PendingHead>(&b) {
                Ok(h) => deadline(h.expires_at, h.dialog_open) < now,
                Err(_) => true, // unreadable: nothing can use it
            },
            // gone meanwhile (or not readable now): nothing to do
            Err(_) => false,
        }
    }

    /// Every change to a pending file happens under this lock, so a read-modify-write never
    /// overwrites a newer file.
    fn lock_pending(&self) -> Result<LockGuard> {
        let path = self.pending_lock();
        lock_file(&path, LOCK_WAIT)?
            .with_context(|| format!("{} is locked by another toastback process", path.display()))
    }

    fn pending_lock(&self) -> PathBuf {
        self.pending_dir().join(".lock")
    }

    /// Write the image read from `src` into the cache as `<id>-<kind>.<ext>`, returning the path.
    pub fn cache_image(&self, id: &str, kind: &str, src: &str, bytes: &[u8]) -> Result<PathBuf> {
        // `bell.` has an empty extension, and Windows would drop the dot from `<id>-icon.`
        let ext = Path::new(src)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty())
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| "png".into());
        self.cache_bytes(id, kind, &ext, bytes)
    }

    /// Write image bytes into the cache as `<id>-<kind>.<ext>`, returning the file's path.
    pub fn cache_bytes(&self, id: &str, kind: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf> {
        fs::create_dir_all(self.cache_dir())?;
        let dest = self.cache_dir().join(format!("{id}-{kind}.{ext}"));
        fs::write(&dest, bytes).with_context(|| format!("write {}", dest.display()))?;
        Ok(dest)
    }

    /// Delete the cached images of these notifications, in one pass over the cache dir. A file
    /// that can't be deleted doesn't stop the others. Returns the ids with an image left, and the
    /// first error.
    fn remove_cached<'a>(&self, ids: &[&'a str]) -> (Vec<&'a str>, Option<anyhow::Error>) {
        let mut stuck = Vec::new();
        let mut first_error = None;
        let entries = match fs::read_dir(self.cache_dir()) {
            Ok(entries) if !ids.is_empty() => entries,
            _ => return (stuck, first_error),
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let owners = ids.iter().copied().filter(|id| is_cached_for(name, id));
            let owners: Vec<&str> = owners.collect();
            if !owners.is_empty()
                && let Err(e) = remove_if_exists(&path)
            {
                stuck.extend(owners);
                first_error.get_or_insert(e);
            }
        }
        (stuck, first_error)
    }

    /// Append an event to the log: `{"ts": …, "event": …, …fields}`. Rotates at ~5 MB.
    pub fn log(&self, event: &str, fields: Value) -> Result<()> {
        let path = self.log_file();
        fs::create_dir_all(path.parent().unwrap())?;
        let too_big = |p: &Path| fs::metadata(p).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES);
        if too_big(&path) {
            // Only one process rotates: without the lock, a second one could rename the fresh
            // log over the rotated one. A failed rotation never costs the event, and one that
            // another process is doing right now needs no waiting for.
            if let Ok(Some(_lock)) = lock_file(&path.with_extension("lock"), Duration::ZERO)
                && too_big(&path)
            {
                let _ = fs::rename(&path, path.with_extension("1.jsonl"));
            }
        }
        let mut line = json!({ "ts": timestamp(SystemTime::now()), "event": event });
        if let (Value::Object(line), Value::Object(fields)) = (&mut line, fields) {
            line.extend(fields);
        }
        // one write per event, so lines from concurrent processes don't interleave
        let mut text = line.to_string();
        text.push('\n');
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        f.write_all(text.as_bytes())?;
        Ok(())
    }

    /// Log events, oldest first (rotated file included). Unparseable lines are skipped.
    pub fn read_log(&self) -> Result<Vec<Value>> {
        let path = self.log_file();
        let mut events = Vec::new();
        for p in [path.with_extension("1.jsonl"), path] {
            let Ok(text) = fs::read_to_string(&p) else {
                continue;
            };
            events.extend(text.lines().filter_map(|l| serde_json::from_str(l).ok()));
        }
        Ok(events)
    }

    pub fn install_record(&self) -> Result<Option<InstallRecord>> {
        match fs::read(self.install_file()) {
            Ok(b) => Ok(Some(
                serde_json::from_slice(&b).context("corrupt install.json")?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("read install.json"),
        }
    }

    pub fn save_install_record(&self, r: &InstallRecord) -> Result<()> {
        write_atomic(&self.install_file(), &serde_json::to_vec_pretty(r)?)
    }

    pub fn remove_install_record(&self) -> Result<()> {
        remove_if_exists(&self.install_file())
    }
}

/// An exclusive OS lock on a file, released when dropped, or by the OS when its holder dies (a
/// killed toastback can't leave it locked). The file itself stays.
struct LockGuard {
    _file: fs::File,
}

/// The lock on `path`, waiting up to `wait` for another process to release it: `None` if it
/// didn't.
fn lock_file(path: &Path, wait: Duration) -> Result<Option<LockGuard>> {
    fs::create_dir_all(path.parent().unwrap())?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(LockGuard { _file: file })),
            Err(fs::TryLockError::WouldBlock) if start.elapsed() < wait => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(e)) => {
                return Err(e).with_context(|| format!("lock {}", path.display()));
            }
        }
    }
}

/// The last second a click still counts: `expires_at`, or a day later for a choice in a dialog
/// that opened before it.
fn deadline(expires_at: u64, dialog_grace: bool) -> u64 {
    if dialog_grace {
        expires_at.saturating_add(DIALOG_GRACE_SECS)
    } else {
        expires_at
    }
}

/// Is `name` a cached image of notification `id`: exactly `<id>-icon.<ext>` or `<id>-image.<ext>`?
/// (Ids may contain `-` and `.`, so a plain prefix would also match `<id>-2-icon.png`.) The id
/// matches in any case, as Windows file names do: `Deploy` and `deploy` share one pending file.
fn is_cached_for(name: &str, id: &str) -> bool {
    let Some(rest) = name
        .get(..id.len())
        .filter(|head| head.eq_ignore_ascii_case(id))
        .map(|_| &name[id.len()..])
    else {
        return false;
    };
    [ICON, IMAGE].iter().any(|kind| {
        rest.strip_prefix('-')
            .and_then(|r| r.strip_prefix(kind))
            .and_then(|r| r.strip_prefix('.'))
            .is_some_and(|ext| !ext.is_empty() && !ext.contains('.'))
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("remove {}", path.display()))
        }
        _ => Ok(()),
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Short, sortable id: milliseconds since the epoch in base 36, then 4 random base-36 chars.
pub fn new_id() -> Result<String> {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let rand = getrandom::u32().map_err(|e| anyhow::anyhow!("OS RNG: {e}"))?;
    Ok(format!(
        "{}-{:0>4}",
        base36(ms),
        base36(u64::from(rand) % 36u64.pow(4))
    ))
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// 128-bit random token from the OS RNG, as 32 hex chars.
pub fn new_token() -> Result<String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("OS RNG: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Compare tokens without an early exit on the first differing byte.
pub fn token_matches(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// UTC timestamp, RFC 3339 with seconds: `2026-10-14T13:20:00Z`.
pub fn timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86400, secs % 86400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let dir =
            std::env::temp_dir().join(format!("toastback-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::at(dir)
    }

    fn pending(id: &str, expires_at: u64) -> Pending {
        Pending {
            spec: Spec {
                id: Some(id.into()),
                title: "t".into(),
                ..Default::default()
            },
            token: new_token().unwrap(),
            created: 0,
            expires_at,
            done: false,
            icon_path: None,
            image_path: None,
            dialog_open: false,
        }
    }

    #[test]
    fn pending_roundtrip_and_prune() {
        let s = temp_store("pending");
        s.save_pending(&pending("old", 100)).unwrap();
        s.save_pending(&pending("new", 300)).unwrap();
        fs::create_dir_all(s.cache_dir()).unwrap();
        fs::write(s.cache_dir().join("old-icon.png"), b"x").unwrap();
        fs::write(s.cache_dir().join("new-icon.png"), b"x").unwrap();
        assert_eq!(s.prune(200).unwrap(), vec!["old".to_string()]);
        assert!(s.load_pending("old").unwrap().is_none());
        assert_eq!(s.load_pending("new").unwrap().unwrap().expires_at, 300);
        assert!(!s.cache_dir().join("old-icon.png").exists());
        assert!(s.cache_dir().join("new-icon.png").exists());
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn cache_files_match_their_id_only() {
        let s = temp_store("cache");
        fs::create_dir_all(s.cache_dir()).unwrap();
        for f in [
            "build-icon.png",
            "build-image.jpg",
            "build-2-icon.png",
            "build.x-icon.png",
        ] {
            fs::write(s.cache_dir().join(f), b"x").unwrap();
        }
        s.save_pending(&pending("build", 100)).unwrap();
        s.remove_pending("build").unwrap();
        assert!(!s.cache_dir().join("build-icon.png").exists());
        assert!(!s.cache_dir().join("build-image.jpg").exists());
        assert!(s.cache_dir().join("build-2-icon.png").exists());
        assert!(s.cache_dir().join("build.x-icon.png").exists());
        assert!(is_cached_for("a-icon.png", "a"));
        assert!(!is_cached_for("a-icon.x-icon.png", "a"));
        assert!(!is_cached_for("a-icon.", "a"));
        // Windows file names ignore case: `Deploy` and `deploy` are one pending file
        assert!(is_cached_for("Deploy-icon.png", "deploy"));
        assert!(!is_cached_for("Deploy-iconx.png", "deploy"));
        assert!(!is_cached_for("d", "deploy"));
        // a source without an extension (or with an empty one) is cached as PNG
        let cached = s.cache_image("n1", ICON, "dir/bell.", b"x").unwrap();
        assert!(cached.ends_with("n1-icon.png"), "{}", cached.display());
        s.remove_pendings(&["n1", "build.x"]).unwrap();
        assert!(!cached.exists());
        assert!(!s.cache_dir().join("build.x-icon.png").exists());
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn ids_from_before_the_leading_dash_rule_still_work() {
        // 1.0.0 accepted ids like `-x`: their notifications stay clickable and removable
        let s = temp_store("legacy");
        let p = pending("-x", 100);
        s.save_pending(&p).unwrap();
        assert!(s.has_pending("-x"));
        assert!(s.load_pending("-x").unwrap().is_some());
        s.remove_pending("-x").unwrap();
        assert!(s.pending_ids().unwrap().is_empty());
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn claim_is_once_per_token() {
        let s = temp_store("claim");
        let p = pending("x", 100);
        s.save_pending(&p).unwrap();
        let claim = |t: &str| s.claim("x", t, 50, false).unwrap();
        assert!(matches!(claim("wrong"), Claim::Replaced));
        assert!(matches!(claim(&p.token), Claim::Claimed));
        assert!(s.load_pending("x").unwrap().unwrap().done);
        assert!(matches!(claim(&p.token), Claim::Done));
        // shown again under the same id: the old token is no use, the new one works
        let again = pending("x", 100);
        s.save_pending(&again).unwrap();
        assert!(matches!(claim(&p.token), Claim::Replaced));
        assert!(matches!(claim(&again.token), Claim::Claimed));
        s.remove_pending("x").unwrap();
        assert!(matches!(claim(&again.token), Claim::Gone));
        // ids that can't name a pending file are never found
        for bad in ["NUL", "com1.json", "a/b", "a b", ""] {
            assert!(
                matches!(s.claim(bad, "t", 50, false).unwrap(), Claim::Gone),
                "{bad}"
            );
            assert!(!s.has_pending(bad));
        }
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn concurrent_claims_have_one_winner() {
        let s = std::sync::Arc::new(temp_store("race"));
        for round in 0..20 {
            let p = pending("x", 100);
            s.save_pending(&p).unwrap();
            let winners: usize = (0..8)
                .map(|_| {
                    let (s, t) = (s.clone(), p.token.clone());
                    std::thread::spawn(move || {
                        matches!(s.claim("x", &t, 50, false).unwrap(), Claim::Claimed)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| usize::from(h.join().unwrap()))
                .sum();
            assert_eq!(winners, 1, "round {round}");
        }
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn late_choices_count_only_for_dialogs_opened_in_time() {
        let s = temp_store("late");
        let toast = pending("toast", 100);
        s.save_pending(&toast).unwrap();
        assert!(matches!(
            s.claim("toast", &toast.token, 101, false).unwrap(),
            Claim::Expired
        ));
        // the last second still counts
        let edge = pending("edge", 100);
        s.save_pending(&edge).unwrap();
        assert!(matches!(
            s.claim("edge", &edge.token, 100, false).unwrap(),
            Claim::Claimed
        ));

        let dlg = pending("dlg", 100);
        s.save_pending(&dlg).unwrap();
        s.set_dialog_open("dlg", &dlg.token, 90).unwrap();
        assert!(matches!(
            s.claim("dlg", &dlg.token, 101 + DIALOG_GRACE_SECS, true)
                .unwrap(),
            Claim::Expired
        ));
        // only the dialog's own choice gets the grace, not a late toast click or a replayed URI
        assert!(matches!(
            s.claim("dlg", &dlg.token, 5000, false).unwrap(),
            Claim::Expired
        ));
        assert!(matches!(
            s.claim("dlg", &dlg.token, 5000, true).unwrap(),
            Claim::Claimed
        ));

        // a dialog that opens after expiry doesn't extend anything
        let late = pending("late", 100);
        s.save_pending(&late).unwrap();
        s.set_dialog_open("late", &late.token, 150).unwrap();
        assert!(!s.load_pending("late").unwrap().unwrap().dialog_open);
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn an_image_that_cant_be_deleted_stops_nothing_else() {
        // a directory can't be removed as a file, like an image another program holds open
        let stuck = |s: &Store, name: &str| fs::create_dir_all(s.cache_dir().join(name)).unwrap();
        let s = temp_store("stuck-prune");
        for id in ["a", "b", "c"] {
            s.save_pending(&pending(id, 100)).unwrap();
        }
        stuck(&s, "b-icon.png");
        fs::write(s.cache_dir().join("a-icon.png"), b"x").unwrap();
        fs::write(s.cache_dir().join("c-icon.png"), b"x").unwrap();
        // the others go, and the stuck one's pending file stays so a later run tries again
        assert_eq!(s.prune(200).unwrap(), ["a", "c"]);
        assert!(!s.cache_dir().join("a-icon.png").exists());
        assert!(!s.cache_dir().join("c-icon.png").exists());
        assert_eq!(s.pending_ids().unwrap(), ["b"]);
        fs::remove_dir(s.cache_dir().join("b-icon.png")).unwrap();
        assert_eq!(s.prune(200).unwrap(), ["b"]);
        fs::remove_dir_all(s.root()).unwrap();

        let s = temp_store("stuck-remove");
        for id in ["a", "b", "c"] {
            s.save_pending(&pending(id, 100)).unwrap();
        }
        stuck(&s, "b-icon.png");
        fs::write(s.cache_dir().join("a-icon.png"), b"x").unwrap();
        fs::write(s.cache_dir().join("c-icon.png"), b"x").unwrap();
        assert!(s.remove_pendings(&["a", "b", "c"]).is_err());
        assert!(s.pending_ids().unwrap().is_empty());
        assert!(!s.cache_dir().join("a-icon.png").exists());
        assert!(!s.cache_dir().join("c-icon.png").exists());
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn a_claim_forgets_env_values() {
        let s = temp_store("claim-env");
        let mut p = pending("x", 100);
        p.spec.default = Some(crate::spec::Run {
            wsl: Some(crate::spec::WslCommand::Shell("gh".into())),
            env: [("GH_TOKEN".to_string(), "ghp_secret".to_string())].into(),
            ..Default::default()
        });
        p.spec.buttons = vec![crate::spec::Button {
            id: "b".into(),
            label: "B".into(),
            run: p.spec.default.clone(),
            ..Default::default()
        }];
        s.save_pending(&p).unwrap();
        let raw = || fs::read_to_string(s.pending_file("x")).unwrap();
        assert!(raw().contains("ghp_secret"));
        assert!(matches!(
            s.claim("x", &p.token, 50, false).unwrap(),
            Claim::Claimed
        ));
        assert!(!raw().contains("ghp_secret"), "{}", raw());
        // still readable, and a second click is still recognized
        assert!(s.load_pending("x").unwrap().unwrap().done);
        assert!(matches!(
            s.claim("x", &p.token, 50, false).unwrap(),
            Claim::Done
        ));
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn prune_keeps_open_dialogs_and_newer_files() {
        let s = temp_store("prune");
        let p = pending("dlg", 100);
        s.save_pending(&p).unwrap();
        s.set_dialog_open("dlg", &p.token, 50).unwrap();
        assert!(s.prune(200).unwrap().is_empty());
        // kept through its last second, like claim counts it
        assert!(s.prune(100 + DIALOG_GRACE_SECS).unwrap().is_empty());
        assert_eq!(s.prune(101 + DIALOG_GRACE_SECS).unwrap(), ["dlg"]);
        // a field from a newer version doesn't make the file unreadable for pruning
        fs::write(
            s.pending_dir().join("future.json"),
            br#"{"expires_at": 500, "new_field": 1, "spec": {"title": "t", "later": true}}"#,
        )
        .unwrap();
        fs::write(s.pending_dir().join("broken.json"), b"{").unwrap();
        assert_eq!(s.prune(200).unwrap(), ["broken"]);
        assert_eq!(s.pending_ids().unwrap(), ["future"]);
        // ... and `remove` still finds it
        assert!(s.has_pending("future"));
        s.remove_pending("future").unwrap();
        assert!(s.pending_ids().unwrap().is_empty());
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn a_held_lock_blocks_and_a_released_one_does_not() {
        let s = temp_store("lock");
        let held = s.lock_pending().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let path = s.pending_lock();
        // a try without waiting gives up at once, and prune leaves its work for the next run
        assert!(lock_file(&path, Duration::ZERO).unwrap().is_none());
        fs::write(
            s.pending_dir().join("old.json"),
            serde_json::to_vec(&pending("old", 100)).unwrap(),
        )
        .unwrap();
        assert!(s.prune(200).unwrap().is_empty());
        let waiter = std::thread::spawn(move || {
            let _l = lock_file(&path, LOCK_WAIT).unwrap().unwrap();
            tx.send(()).unwrap();
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "got the lock while held"
        );
        drop(held);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        waiter.join().unwrap();
        assert_eq!(s.prune(200).unwrap(), ["old"]);
        // the lock file stays behind; that doesn't lock anything
        assert!(s.pending_dir().join(".lock").exists());
        s.save_pending(&pending("x", 100)).unwrap();
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn log_appends_and_reads() {
        let s = temp_store("log");
        s.log("shown", json!({"id": "a"})).unwrap();
        s.log("activated", json!({"id": "a", "action": "b"}))
            .unwrap();
        let events = s.read_log().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["event"], "activated");
        assert_eq!(events[1]["action"], "b");
        assert!(events[0]["ts"].as_str().unwrap().ends_with('Z'));
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn failed_rotation_still_logs() {
        // like losing the rotation race: the rename fails, the event is written anyway
        let s = temp_store("rotate");
        fs::create_dir_all(s.log_file().with_extension("1.jsonl")).unwrap();
        fs::write(s.log_file(), vec![b'\n'; LOG_ROTATE_BYTES as usize + 1]).unwrap();
        s.log("shown", json!({"id": "a"})).unwrap();
        assert_eq!(s.read_log().unwrap().len(), 1);
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn shims_declined_reads_old_and_new_records() {
        let record = |json: &str| serde_json::from_str::<InstallRecord>(json).unwrap();
        let base = r#""version": "1", "installed_at": "t", "exe": "e""#;
        let shim = r#""shims": [{"distro": "D", "user": null}]"#;
        // written before `no_shims` existed
        assert!(record(&format!("{{{base}}}")).shims_declined());
        assert!(!record(&format!("{{{base}, {shim}}}")).shims_declined());
        // written since: says so itself
        assert!(!record(&format!(r#"{{{base}, "no_shims": false}}"#)).shims_declined());
        assert!(record(&format!(r#"{{{base}, {shim}, "no_shims": true}}"#)).shims_declined());
    }

    #[test]
    fn ids_and_tokens() {
        let a = new_id().unwrap();
        std::thread::sleep(Duration::from_millis(2));
        let b = new_id().unwrap();
        assert!(crate::spec::is_token(&a, 64));
        assert!(a < b, "{a} < {b}");
        let t = new_token().unwrap();
        assert_eq!(t.len(), 32);
        assert!(token_matches(&t, &t.clone()));
        assert!(!token_matches(&t, &new_token().unwrap()));
        assert!(!token_matches(&t, ""));
    }

    #[test]
    fn timestamps() {
        let at = |s| timestamp(UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(at(1_791_984_212), "2026-10-14T13:23:32Z");
    }
}
