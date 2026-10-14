// SPDX-License-Identifier: GPL-3.0-or-later
//! State in `%LOCALAPPDATA%\toastback`: pending notifications, the event log, the install record.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::spec::Spec;

const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

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
    #[serde(default)]
    pub path_added: bool,
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

    pub fn save_pending(&self, p: &Pending) -> Result<()> {
        let id = p.spec.id.as_deref().context("pending spec without id")?;
        write_atomic(&self.pending_file(id), &serde_json::to_vec_pretty(p)?)
    }

    /// `None` when there is no such pending notification (expired, pruned or never shown).
    pub fn load_pending(&self, id: &str) -> Result<Option<Pending>> {
        match fs::read(self.pending_file(id)) {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes).context("corrupt pending file")?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("read pending file"),
        }
    }

    pub fn remove_pending(&self, id: &str) -> Result<()> {
        remove_if_exists(&self.pending_file(id))?;
        self.remove_cached(id)
    }

    /// Delete pending files (and their cached images) whose notification expired. Returns the ids.
    pub fn prune(&self, now: u64) -> Result<Vec<String>> {
        let mut pruned = Vec::new();
        let entries = match fs::read_dir(self.pending_dir()) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(pruned),
            Err(e) => return Err(e).context("read pending dir"),
        };
        for entry in entries {
            let path = entry?.path();
            let Some(id) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".json"))
            else {
                continue;
            };
            let expired = match fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Pending>(&b).ok())
            {
                Some(p) => p.expires_at <= now,
                None => true, // unreadable: nothing can use it
            };
            if expired {
                let id = id.to_string();
                self.remove_pending(&id)?;
                pruned.push(id);
            }
        }
        Ok(pruned)
    }

    /// Copy an image into the cache as `<id>-<kind>.<ext>`, returning the copy's path.
    pub fn cache_image(&self, id: &str, kind: &str, src: &str) -> Result<PathBuf> {
        let bytes = fs::read(src).with_context(|| format!("{kind}: cannot read {src}"))?;
        let ext = Path::new(src)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| "png".into());
        fs::create_dir_all(self.cache_dir())?;
        let dest = self.cache_dir().join(format!("{id}-{kind}.{ext}"));
        fs::write(&dest, bytes).with_context(|| format!("write {}", dest.display()))?;
        Ok(dest)
    }

    fn remove_cached(&self, id: &str) -> Result<()> {
        let Ok(entries) = fs::read_dir(self.cache_dir()) else {
            return Ok(());
        };
        let prefix = format!("{id}-");
        for entry in entries {
            let path = entry?.path();
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
            {
                remove_if_exists(&path)?;
            }
        }
        Ok(())
    }

    /// Append an event to the log: `{"ts": …, "event": …, …fields}`. Rotates at ~5 MB.
    pub fn log(&self, event: &str, fields: Value) -> Result<()> {
        let path = self.log_file();
        fs::create_dir_all(path.parent().unwrap())?;
        if fs::metadata(&path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
            fs::rename(&path, path.with_extension("1.jsonl"))?;
        }
        let mut line = json!({ "ts": timestamp(SystemTime::now()), "event": event });
        if let (Value::Object(line), Value::Object(fields)) = (&mut line, fields) {
            line.extend(fields);
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(f, "{line}")?;
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
    use std::time::Duration;

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
