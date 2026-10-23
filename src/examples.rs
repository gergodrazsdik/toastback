// SPDX-License-Identifier: GPL-3.0-or-later
//! `examples`: the built-in example specs, embedded from `examples/*.json`, so an installed
//! toastback can print them without the repository.

pub struct Example {
    pub name: &'static str,
    pub description: &'static str,
    pub json: &'static str,
}

/// Simplest first. Every `examples/*.json` is either here or in [`REPO_ONLY`].
pub const BUILTIN: [Example; 7] = [
    Example {
        name: "simple",
        description: "a plain toast: title, body and a source line, no sound",
        json: include_str!("../examples/simple.json"),
    },
    Example {
        name: "warning",
        description: "a built-in icon: info, warning, error or question",
        json: include_str!("../examples/warning.json"),
    },
    Example {
        name: "build",
        description: "build failed: open the log or re-run; a new one replaces it (same id)",
        json: include_str!("../examples/build.json"),
    },
    Example {
        name: "deploy",
        description: "a question with details and data; the button runs a WSL command",
        json: include_str!("../examples/deploy.json"),
    },
    Example {
        name: "export",
        description: "buttons that run on Windows: open a folder, PowerShell",
        json: include_str!("../examples/export.json"),
    },
    Example {
        name: "reboot-dialog",
        description: "a dialog that waits for an answer, with remind-me-later",
        json: include_str!("../examples/reboot-dialog.json"),
    },
    Example {
        name: "alarm",
        description: "stays on screen and loops its sound until you join or snooze",
        json: include_str!("../examples/alarm.json"),
    },
];

/// Feature and test specs that stay in the repository: they need its files (`images`) or exist
/// for docs/manual-test.md rather than as something to copy.
#[cfg(test)]
const REPO_ONLY: [&str; 5] = [
    "images",
    "default-handler",
    "feedback",
    "wsl-options",
    "windows-callbacks",
];

pub fn find(name: &str) -> Option<&'static Example> {
    BUILTIN.iter().find(|e| e.name == name)
}

/// The list `toastback examples` prints: names, descriptions and how to use one.
pub fn list() -> String {
    let width = BUILTIN.iter().map(|e| e.name.len()).max().unwrap_or(0);
    let mut s = String::new();
    for e in &BUILTIN {
        s += &format!("{:width$}  {}\n", e.name, e.description);
    }
    s += "\nPrint one:  toastback examples deploy > deploy.json\n";
    s += "Show one:   toastback examples deploy | toastback\n";
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Spec;
    use std::collections::HashSet;

    #[test]
    fn builtins_are_valid_specs() {
        let mut names = HashSet::new();
        for e in &BUILTIN {
            assert!(names.insert(e.name), "duplicate example {}", e.name);
            let spec = Spec::parse(e.json).unwrap_or_else(|err| panic!("{}: {err:#}", e.name));
            spec.validate()
                .unwrap_or_else(|err| panic!("{}: {err:#}", e.name));
            assert!(e.json.ends_with('\n'), "{} lacks a final newline", e.name);
        }
        assert!(find("deploy").is_some());
        assert!(find("images").is_none());
        assert!(
            list()
                .lines()
                .any(|l| l.starts_with("reboot-dialog  a dialog"))
        );
    }

    #[test]
    fn every_repo_example_is_builtin_or_repo_only() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".json").map(str::to_string)
            })
            .collect();
        files.sort();
        let mut known: Vec<String> = BUILTIN
            .iter()
            .map(|e| e.name)
            .chain(REPO_ONLY)
            .map(str::to_string)
            .collect();
        known.sort();
        assert_eq!(files, known, "register new examples in src/examples.rs");
    }
}
