//! Remembering the window position through a KDE window rule.
//!
//! Wayland apps cannot read or set their own position; the compositor decides. `KWin` can
//! remember it per application, which is what this rule asks for. It is stored in
//! `~/.config/kwinrulesrc` and edited with KDE's own `kwriteconfig6`, so the file keeps its
//! format. The rule only affects windows whose class is `mer`.

use std::path::PathBuf;
use std::process::Command;

const RULE_ID: &str = "mer-remember-position";
const FILE: &str = "kwinrulesrc";

fn rules_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join(FILE))
}

/// `rules=a,b` value with our id added (once).
fn rules_with(current: &str) -> String {
    let mut ids: Vec<&str> = current.split(',').filter(|id| !id.is_empty()).collect();
    if !ids.contains(&RULE_ID) {
        ids.push(RULE_ID);
    }
    ids.join(",")
}

/// `rules=a,b` value without our id.
fn rules_without(current: &str) -> String {
    current.split(',').filter(|id| !id.is_empty() && *id != RULE_ID).collect::<Vec<_>>().join(",")
}

/// The `rules=` value of the `[General]` group in the text of a `kwinrulesrc`.
fn general_rules(text: &str) -> String {
    let mut in_general = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_general = line == "[General]";
        } else if in_general && let Some(value) = line.strip_prefix("rules=") {
            return value.trim().to_owned();
        }
    }
    String::new()
}

/// Is the rule installed?
pub fn is_enabled() -> bool {
    rules_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|text| general_rules(&text).split(',').any(|id| id == RULE_ID))
}

fn write(group: &str, key: &str, value: &str) -> Result<(), String> {
    let status = Command::new("kwriteconfig6")
        .args(["--file", FILE, "--group", group, "--key", key, value])
        .status()
        .map_err(|error| format!("cannot run kwriteconfig6 (is this KDE?): {error}"))?;
    status.success().then_some(()).ok_or_else(|| format!("kwriteconfig6 failed on `{key}`"))
}

/// Asks `KWin` to reload its rules. Best effort: it also picks them up on its own later.
fn reconfigure() {
    let _ = Command::new("busctl")
        .args(["--user", "call", "org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure"])
        .status();
}

fn current_rules() -> String {
    rules_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| general_rules(&text))
        .unwrap_or_default()
}

/// Installs the rule: `KWin` remembers where the window was, per its class.
pub fn enable() -> Result<(), String> {
    write(RULE_ID, "Description", "mer: remember window position")?;
    write(RULE_ID, "wmclass", "mer")?;
    write(RULE_ID, "wmclasscomplete", "false")?;
    write(RULE_ID, "wmclassmatch", "1")?;
    // 4 = "Remember": `KWin` stores the position when the window closes and applies it next time.
    write(RULE_ID, "positionrule", "4")?;
    write("General", "rules", &rules_with(&current_rules()))?;
    let count = rules_with(&current_rules()).split(',').count();
    write("General", "count", &count.to_string())?;
    reconfigure();
    Ok(())
}

/// Removes the rule and its settings.
pub fn disable() -> Result<(), String> {
    let remaining = rules_without(&current_rules());
    write("General", "rules", &remaining)?;
    let count = remaining.split(',').filter(|id| !id.is_empty()).count();
    write("General", "count", &count.to_string())?;
    // Deleting a whole group: kwriteconfig6 has no verb for it, so clear what we set.
    for key in
        ["Description", "wmclass", "wmclasscomplete", "wmclassmatch", "positionrule", "position"]
    {
        let _ = Command::new("kwriteconfig6")
            .args(["--file", FILE, "--group", RULE_ID, "--key", key, "--delete"])
            .status();
    }
    reconfigure();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_id_is_added_once_and_keeps_the_others() {
        assert_eq!(rules_with(""), RULE_ID);
        assert_eq!(rules_with("a,b"), format!("a,b,{RULE_ID}"));
        assert_eq!(rules_with(&rules_with("a")), format!("a,{RULE_ID}"));
    }

    #[test]
    fn our_id_is_removed_and_the_others_stay() {
        assert_eq!(rules_without(&format!("a,{RULE_ID},b")), "a,b");
        assert_eq!(rules_without(RULE_ID), "");
        assert_eq!(rules_without("a"), "a");
    }

    #[test]
    fn the_general_rules_line_is_found() {
        let text = format!("[General]\ncount=1\nrules={RULE_ID}\n\n[{RULE_ID}]\nrules=nope\n");
        assert_eq!(general_rules(&text), RULE_ID);
        assert_eq!(general_rules("[Other]\nrules=x\n"), "");
        assert_eq!(general_rules(""), "");
    }
}
