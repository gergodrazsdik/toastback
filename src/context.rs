// SPDX-License-Identifier: GPL-3.0-or-later
//! Turning the paths a WSL caller passes (icons, images) into Windows paths.

use anyhow::{Result, bail};

use crate::spec::Context;

/// True for `C:\…`, `C:/…` and UNC paths (`\\server\…`).
pub fn is_windows_path(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
        || p.starts_with(r"\\")
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
