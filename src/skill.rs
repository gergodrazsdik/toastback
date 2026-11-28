// SPDX-License-Identifier: GPL-3.0-or-later
//! `skill`: the agent skill (`skills/toastback/SKILL.md`), embedded so that the installed exe prints, and
//! `install --skill` writes, the one that matches its version (ADR 0021).

const TEMPLATE: &str = include_str!("../skills/toastback/SKILL.md");

/// A line near the top (after the frontmatter, which must come first): `install` replaces and
/// `uninstall` deletes only a `SKILL.md` that carries it.
pub const MARKER: &str = "<!-- toastback skill";

/// Where `install --skill` writes it, relative to `$HOME`.
pub const SKILL_PATH: &str = ".claude/skills/toastback/SKILL.md";

/// The skill for this version, with LF line endings.
pub fn render(version: &str) -> String {
    TEMPLATE.replace('\r', "").replace("{{VERSION}}", version)
}

/// Does this file carry [`MARKER`] where `install` looks for it?
pub fn is_ours(content: &str) -> bool {
    content
        .lines()
        .take(MARKER_LINES)
        .any(|l| l.contains(MARKER))
}

/// How many lines from the top [`MARKER`] may be: the frontmatter comes first.
pub const MARKER_LINES: usize = 12;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use crate::spec::{Button, Progress, ProgressValue, Run, Spec};
    use clap::CommandFactory;
    use std::collections::BTreeSet;

    /// The text in code: fenced blocks and `inline` spans, where commands, flags and fields are named.
    fn code(md: &str) -> String {
        let mut out = String::new();
        let mut fenced = false;
        for line in md.lines() {
            if line.starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                out += line;
                out.push('\n');
            } else {
                for (i, span) in line.split('`').enumerate() {
                    if i % 2 == 1 {
                        out += span;
                        out.push('\n');
                    }
                }
            }
        }
        out
    }

    /// The fenced ```json blocks.
    fn json_blocks(md: &str) -> Vec<String> {
        let mut blocks = Vec::new();
        let mut current: Option<String> = None;
        for line in md.lines() {
            match (&mut current, line) {
                (None, "```json") => current = Some(String::new()),
                (Some(b), "```") => {
                    blocks.push(std::mem::take(b));
                    current = None;
                }
                (Some(b), l) => {
                    *b += l;
                    b.push('\n');
                }
                (None, _) => {}
            }
        }
        blocks
    }

    /// Words that follow `toastback ` in code, and `--long` flags.
    fn mentions(code: &str) -> (BTreeSet<String>, BTreeSet<String>) {
        let word = |s: &str| -> String {
            s.chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-' || c.is_ascii_digit())
                .collect()
        };
        let mut commands = BTreeSet::new();
        for (i, _) in code.match_indices("toastback ") {
            // `~/.local/bin/toastback ` and `| toastback ` count; `toastback.exe` and `toastback-cli` don't
            let w = word(&code[i + "toastback ".len()..]);
            if !w.is_empty() && !w.starts_with('-') {
                commands.insert(w);
            }
        }
        let mut flags = BTreeSet::new();
        for (i, _) in code.match_indices("--") {
            let w = word(&code[i + 2..]);
            if w.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                flags.insert(format!("--{w}"));
            }
        }
        (commands, flags)
    }

    /// Visible subcommands, and every long flag (with the subcommands' own) that isn't hidden.
    fn cli() -> (BTreeSet<String>, BTreeSet<String>) {
        let cmd = Cli::command();
        let visible: Vec<&clap::Command> =
            cmd.get_subcommands().filter(|c| !c.is_hide_set()).collect();
        let commands = visible.iter().map(|c| c.get_name().to_string()).collect();
        let flags = std::iter::once(&cmd)
            .chain(visible.iter().copied())
            .flat_map(|c| c.get_arguments())
            .filter(|a| !a.is_hide_set())
            .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
            .collect();
        (commands, flags)
    }

    #[test]
    fn renders() {
        let s = render("1.6.0");
        assert!(s.starts_with("---\nname: toastback\ndescription: "));
        assert!(is_ours(&s));
        assert!(s.contains("toastback 1.6.0."));
        assert!(!s.contains("{{") && !s.contains('\r'));
        // it goes into a heredoc in the install script
        assert!(!s.lines().any(|l| l == "TOASTBACK_EOF"));
        assert!(!is_ours("---\nname: mine\n---\n# mine\n"));
    }

    #[test]
    fn names_only_commands_and_flags_that_exist() {
        let (commands, flags) = mentions(&code(TEMPLATE));
        let (known_commands, known_flags) = cli();
        let unknown: Vec<_> = commands.difference(&known_commands).collect();
        assert!(
            unknown.is_empty(),
            "the skill names commands that don't exist: {unknown:?}"
        );
        let unknown: Vec<_> = flags
            .iter()
            .filter(|f| !known_flags.contains(*f) && !["--help", "--version"].contains(&f.as_str()))
            .collect();
        assert!(
            unknown.is_empty(),
            "the skill names flags that don't exist: {unknown:?}"
        );
    }

    #[test]
    fn covers_every_command_and_flag() {
        let (commands, flags) = mentions(&code(TEMPLATE));
        let (known_commands, known_flags) = cli();
        let missing: Vec<_> = known_commands.difference(&commands).collect();
        assert!(
            missing.is_empty(),
            "add these commands to the skill: {missing:?}"
        );
        let missing: Vec<_> = known_flags.difference(&flags).collect();
        assert!(
            missing.is_empty(),
            "add these flags to the skill: {missing:?}"
        );
    }

    #[test]
    fn its_specs_are_valid() {
        let blocks = json_blocks(TEMPLATE);
        assert!(!blocks.is_empty());
        for b in &blocks {
            let spec = Spec::parse(b).unwrap_or_else(|e| panic!("{e:#}\n{b}"));
            spec.validate().unwrap_or_else(|e| panic!("{e:#}\n{b}"));
        }
    }

    #[test]
    fn covers_every_spec_field() {
        let keys = |v: serde_json::Value| -> Vec<String> {
            v.as_object().unwrap().keys().cloned().collect()
        };
        let progress = Progress {
            value: ProgressValue::Fraction(0.0),
            status: None,
            title: None,
            label: None,
        };
        let fields = [
            keys(serde_json::to_value(Spec::default()).unwrap()),
            keys(serde_json::to_value(Run::default()).unwrap()),
            keys(serde_json::to_value(Button::default()).unwrap()),
            keys(serde_json::to_value(progress).unwrap()),
            keys(serde_json::to_value(crate::spec::Context::default()).unwrap()),
        ];
        let missing: Vec<&String> = fields
            .iter()
            .flatten()
            .filter(|f| {
                !TEMPLATE.contains(&format!("`{f}`")) && !TEMPLATE.contains(&format!("\"{f}\""))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "add these spec fields to the skill: {missing:?}"
        );
    }

    #[test]
    fn lists_every_exit_code() {
        let section = TEMPLATE
            .split("## Exit codes")
            .nth(1)
            .and_then(|s| s.split("\n## ").next())
            .expect("an Exit codes section");
        let listed: BTreeSet<u8> = section
            .lines()
            .filter_map(|l| l.strip_prefix("| ")?.split(' ').next()?.parse().ok())
            .collect();
        let codes = BTreeSet::from([
            0,
            crate::notify_send::EXIT_ERROR,
            crate::show::EXIT_INVALID,
            crate::show::EXIT_SHOW_FAILED,
            crate::show::EXIT_NOT_INSTALLED,
            crate::wait::EXIT_CLOSED,
            crate::wait::EXIT_TIMED_OUT,
            crate::wait::EXIT_INTERRUPTED,
        ]);
        assert_eq!(listed, codes);
    }

    #[test]
    fn mentions_parses_code() {
        let md = "Run `toastback wait ID --timeout 5m`, not toastback can.\n```sh\n\
                  x | toastback --wait\n~/.local/bin/toastback -t x\ntoastback.exe install\n```\n";
        let (commands, flags) = mentions(&code(md));
        assert_eq!(commands, BTreeSet::from(["wait".to_string()]));
        assert_eq!(
            flags,
            BTreeSet::from(["--timeout".to_string(), "--wait".to_string()])
        );
    }
}
