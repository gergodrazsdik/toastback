// SPDX-License-Identifier: GPL-3.0-or-later
//! Turning the paths a WSL caller passes (icons, images) into Windows paths.

use anyhow::{Result, bail};

use crate::spec::Context;

/// WSL distro names are case-insensitive (`wsl -d ubuntu` runs Ubuntu).
pub fn same_distro(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Is `target` a distro other than the caller's? Only when both are known: a caller whose distro
/// is unknown (no shim, or one that couldn't tell) keeps the user and directory it passed.
pub fn is_other_distro(target: Option<&str>, caller: Option<&str>) -> bool {
    target.zip(caller).is_some_and(|(t, c)| !same_distro(t, c))
}

/// True for `C:\…`, `C:/…` and UNC paths (`\\server\…`).
pub fn is_windows_path(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
        || p.starts_with(r"\\")
}

/// A `cwd` that needs no caller directory: a Linux (`/…`) or Windows path, or the home
/// directory (`~…`, which only `wsl` callbacks accept).
pub fn is_absolute(p: &str) -> bool {
    p.starts_with('/') || p.starts_with('~') || is_windows_path(p)
}

/// Convert a path from the spec to one Windows can open.
///
/// Windows paths pass through. `/mnt/<drive>/…` becomes `<DRIVE>:\…`. Other absolute Linux paths
/// become `\\wsl.localhost\<distro>\…`, and relative ones are resolved against the caller's cwd first.
pub fn to_windows_path(p: &str, ctx: &Context) -> Result<String> {
    if is_windows_path(p) {
        return Ok(p.to_string());
    }
    let abs = if p.starts_with('/') {
        p.to_string()
    } else {
        let Some(cwd) = &ctx.cwd else {
            // no caller context (run without the shim): relative to this process's cwd
            return Ok(p.replace('/', "\\"));
        };
        // a spec's `context` may name a Windows directory; `wsl` callbacks accept one too
        if is_windows_path(cwd) {
            let dir = cwd.trim_end_matches(['\\', '/']);
            return Ok(format!(r"{dir}\{}", p.replace('/', "\\")));
        }
        if !cwd.starts_with('/') {
            bail!("{p:?} is relative, and {cwd:?} isn't a directory it can be resolved against");
        }
        format!("{cwd}/{p}")
    };
    let parts = normalize(&abs);
    if let [mnt, drive, rest @ ..] = parts.as_slice()
        && *mnt == "mnt"
        && drive.len() == 1
        && drive.as_bytes()[0].is_ascii_alphabetic()
    {
        return Ok(format!(
            "{}:\\{}",
            drive.to_ascii_uppercase(),
            rest.join("\\")
        ));
    }
    let Some(distro) = &ctx.distro else {
        bail!(
            "{p:?} is a Linux path and the distro is unknown (run through the WSL shim or pass --distro)"
        );
    };
    Ok(format!(r"\\wsl.localhost\{distro}\{}", parts.join("\\")))
}

/// Split an absolute Linux path into components, resolving `.` and `..`.
fn normalize(abs: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    for part in abs.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context {
            distro: Some("Ubuntu-24.04".into()),
            user: Some("g".into()),
            cwd: Some("/home/g/app".into()),
        }
    }

    #[test]
    fn conversions() {
        let c = ctx();
        let cases = [
            (r"C:\icons\a.png", r"C:\icons\a.png"),
            ("D:/x.png", "D:/x.png"),
            (r"\\server\share\a.png", r"\\server\share\a.png"),
            ("/mnt/c/Users/g/a.png", r"C:\Users\g\a.png"),
            (
                "/home/g/icons/rocket.png",
                r"\\wsl.localhost\Ubuntu-24.04\home\g\icons\rocket.png",
            ),
            (
                "icons/./a.png",
                r"\\wsl.localhost\Ubuntu-24.04\home\g\app\icons\a.png",
            ),
            (
                "../shared/a.png",
                r"\\wsl.localhost\Ubuntu-24.04\home\g\shared\a.png",
            ),
            (
                "/mnt/wsl/x.png",
                r"\\wsl.localhost\Ubuntu-24.04\mnt\wsl\x.png",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(to_windows_path(input, &c).unwrap(), want, "{input}");
        }
    }

    #[test]
    fn caller_directory_forms() {
        let at = |cwd: &str| Context {
            distro: Some("D".into()),
            user: None,
            cwd: Some(cwd.into()),
        };
        assert_eq!(
            to_windows_path("sub/a.png", &at(r"C:\proj\")).unwrap(),
            r"C:\proj\sub\a.png"
        );
        assert_eq!(
            to_windows_path("a.png", &at(r"\\srv\share")).unwrap(),
            r"\\srv\share\a.png"
        );
        // absolute paths don't need it
        assert_eq!(
            to_windows_path("/home/a.png", &at("~/proj")).unwrap(),
            r"\\wsl.localhost\D\home\a.png"
        );
        for cwd in ["~/proj", "proj"] {
            assert!(to_windows_path("a.png", &at(cwd)).is_err(), "{cwd}");
        }
        // no distro is needed for a Windows directory
        let no_distro = Context {
            cwd: Some(r"C:\proj".into()),
            ..Default::default()
        };
        assert_eq!(
            to_windows_path("a.png", &no_distro).unwrap(),
            r"C:\proj\a.png"
        );
    }

    #[test]
    fn needs_context() {
        let empty = Context::default();
        assert!(to_windows_path("/home/g/a.png", &empty).is_err());
        assert_eq!(
            to_windows_path("icons/a.png", &empty).unwrap(),
            r"icons\a.png"
        );
        assert_eq!(
            to_windows_path("/mnt/c/a.png", &empty).unwrap(),
            r"C:\a.png"
        );
    }
}
