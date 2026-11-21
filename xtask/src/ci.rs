//! `cargo xtask ci`: the checks CI runs, so they can be run locally first.
use std::process::Command;

use anyhow::{Result, bail};

/// Each check: a label and the `cargo` arguments.
const CHECKS: &[(&str, &[&str])] = &[
    ("format", &["fmt", "--all", "--check"]),
    ("clippy", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
    ("tests", &["test", "--workspace"]),
];

pub fn run() -> Result<()> {
    let mut failed = Vec::new();
    for (label, args) in CHECKS {
        println!("==> {label}: cargo {}", args.join(" "));
        let status =
            Command::new("cargo").args(*args).current_dir(crate::workspace_root()).status()?;
        if !status.success() {
            failed.push(*label);
        }
    }
    if failed.is_empty() {
        println!("all checks passed");
        Ok(())
    } else {
        bail!("failed: {}", failed.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::CHECKS;

    #[test]
    fn clippy_denies_warnings_like_ci_does() {
        let clippy = CHECKS.iter().find(|(label, _)| *label == "clippy").expect("has clippy");
        assert!(clippy.1.windows(2).any(|pair| pair == ["-D", "warnings"]));
    }
}
