// SPDX-License-Identifier: GPL-3.0-or-later
//! Embeds the Windows resources (icon, manifest, version info) and the git sha.

use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=res");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");

    let sha = env::var("GITHUB_SHA")
        .ok()
        .map(|s| s.chars().take(7).collect())
        .or_else(git_sha)
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=TOASTBACK_GIT_SHA={sha}");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let macros = [
        format!("VER_MAJOR={}", env::var("CARGO_PKG_VERSION_MAJOR").unwrap()),
        format!("VER_MINOR={}", env::var("CARGO_PKG_VERSION_MINOR").unwrap()),
        format!("VER_PATCH={}", env::var("CARGO_PKG_VERSION_PATCH").unwrap()),
        format!("VER_STR=\"{}\"", env::var("CARGO_PKG_VERSION").unwrap()),
    ];
    embed_resource::compile("res/toastback.rc", &macros)
        .manifest_required()
        .unwrap();
}

fn git_sha() -> Option<String> {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
