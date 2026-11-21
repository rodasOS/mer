//! Reading hook payloads and installing the hooks into the agents' own settings.

use serde_json::{Value, json};

use crate::HookKind;

#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("settings are not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("settings are not valid TOML: {0}")]
    Toml(#[from] toml_edit::TomlError),
    #[error("unexpected settings shape: {0}")]
    Shape(&'static str),
}

/// Claude Code passes a JSON object on stdin with `hook_event_name` and `session_id`.
pub fn parse_claude_payload(json_text: &str) -> Option<(HookKind, Option<String>)> {
    let value: Value = serde_json::from_str(json_text).ok()?;
    let session = value.get("session_id").and_then(Value::as_str).map(str::to_owned);
    let kind = match value.get("hook_event_name")?.as_str()? {
        "SessionStart" => HookKind::SessionStart,
        // A tool ran: after the user approved a permission the agent is working again.
        "UserPromptSubmit" | "PostToolUse" => HookKind::Working,
        "Notification" => notification_kind(&value)?,
        // `SubagentStop` is not the end of the main agent's turn.
        "Stop" => HookKind::Done,
        _ => return None,
    };
    Some((kind, session))
}

/// A Claude `Notification` is only news when the agent needs the user. The same hook also
/// fires for "still waiting for your input" after a minute of silence, which is not.
fn notification_kind(value: &Value) -> Option<HookKind> {
    match value.get("notification_type").and_then(Value::as_str) {
        Some("permission_prompt" | "elicitation_dialog") => Some(HookKind::Waiting),
        Some(_) => None,
        // Older versions send only the message.
        None => {
            let message = value.get("message").and_then(Value::as_str).unwrap_or_default();
            if message.contains("waiting for your input") { None } else { Some(HookKind::Waiting) }
        }
    }
}

/// Codex calls the `notify` program with one JSON argument.
pub fn parse_codex_payload(json_text: &str) -> Option<(HookKind, Option<String>)> {
    let value: Value = serde_json::from_str(json_text).ok()?;
    let session = value
        .get("thread-id")
        .or_else(|| value.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    match value.get("type")?.as_str()? {
        "agent-turn-complete" => Some((HookKind::Done, session)),
        _ => None,
    }
}

const CLAUDE_EVENTS: [&str; 5] =
    ["SessionStart", "UserPromptSubmit", "PostToolUse", "Notification", "Stop"];

fn is_mer_command(entry: &Value, command: &str) -> bool {
    entry.get("hooks").and_then(Value::as_array).is_some_and(|hooks| {
        hooks.iter().any(|hook| hook.get("command").and_then(Value::as_str) == Some(command))
    })
}

/// Adds `mer`'s hooks to a Claude Code `settings.json`. Idempotent; other hooks are kept.
/// Returns the new settings and whether anything changed.
pub fn patch_claude_settings(
    mut settings: Value,
    command: &str,
) -> Result<(Value, bool), HookError> {
    let root = settings.as_object_mut().ok_or(HookError::Shape("settings must be an object"))?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().ok_or(HookError::Shape("`hooks` must be an object"))?;

    let mut changed = false;
    for event in CLAUDE_EVENTS {
        let list = hooks.entry(event).or_insert_with(|| json!([]));
        let list = list.as_array_mut().ok_or(HookError::Shape("a hook event must be an array"))?;
        if !list.iter().any(|entry| is_mer_command(entry, command)) {
            list.push(json!({ "hooks": [{ "type": "command", "command": command }] }));
            changed = true;
        }
    }
    Ok((settings, changed))
}

/// Sets Codex's `notify` to call `mer`, keeping the rest of `config.toml` (and its comments).
/// An existing `notify` that is not ours is never overwritten.
pub fn patch_codex_config(text: &str, program: &str) -> Result<(String, bool), HookError> {
    let mut document: toml_edit::DocumentMut = text.parse()?;
    if let Some(existing) = document.get("notify") {
        let ours = existing
            .as_array()
            .is_some_and(|array| array.iter().any(|item| item.as_str() == Some("hook")));
        return if ours {
            Ok((text.to_owned(), false))
        } else {
            Err(HookError::Shape("`notify` is already set to another program"))
        };
    }
    let mut array = toml_edit::Array::new();
    for part in [program, "hook", "--agent", "codex"] {
        array.push(part);
    }
    document["notify"] = toml_edit::value(array);
    Ok((document.to_string(), true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_events_map_to_hook_kinds() {
        let parse = |name: &str| {
            parse_claude_payload(&format!(r#"{{"hook_event_name":"{name}","session_id":"abc"}}"#))
        };
        assert_eq!(parse("UserPromptSubmit"), Some((HookKind::Working, Some("abc".to_owned()))));
        assert_eq!(parse("PostToolUse"), Some((HookKind::Working, Some("abc".to_owned()))));
        assert_eq!(parse("Notification"), Some((HookKind::Waiting, Some("abc".to_owned()))));
        assert_eq!(parse("Stop"), Some((HookKind::Done, Some("abc".to_owned()))));
        assert_eq!(parse("SubagentStop"), None);
        assert_eq!(parse("SessionStart"), Some((HookKind::SessionStart, Some("abc".to_owned()))));
        assert_eq!(parse("PreToolUse"), None);
        assert_eq!(parse_claude_payload("not json"), None);
    }

    #[test]
    fn only_notifications_that_need_the_user_are_waiting() {
        let parse = |extra: &str| {
            parse_claude_payload(&format!(
                r#"{{"hook_event_name":"Notification","session_id":"s"{extra}}}"#
            ))
            .map(|(kind, _)| kind)
        };
        assert_eq!(parse(r#","notification_type":"permission_prompt""#), Some(HookKind::Waiting));
        assert_eq!(parse(r#","notification_type":"elicitation_dialog""#), Some(HookKind::Waiting));
        assert_eq!(parse(r#","notification_type":"idle_prompt""#), None);
        assert_eq!(parse(r#","notification_type":"auth_success""#), None);
        // No type: decide by the message.
        assert_eq!(
            parse(r#","message":"Claude needs your permission to use Bash""#),
            Some(HookKind::Waiting)
        );
        assert_eq!(parse(r#","message":"Claude is waiting for your input""#), None);
        assert_eq!(parse(""), Some(HookKind::Waiting));
    }

    #[test]
    fn approving_a_permission_returns_to_working() {
        use crate::AgentStatus;
        let mut status = AgentStatus::default();
        for (event, expected) in [
            ("UserPromptSubmit", AgentStatus::Working),
            ("Notification", AgentStatus::Waiting),
            ("PostToolUse", AgentStatus::Working),
            ("Stop", AgentStatus::Done),
        ] {
            let payload = format!(
                r#"{{"hook_event_name":"{event}","notification_type":"permission_prompt"}}"#
            );
            let (kind, _) = parse_claude_payload(&payload).expect("known event");
            status = status.after(kind);
            assert_eq!(status, expected, "after {event}");
        }
    }

    #[test]
    fn an_old_settings_file_gets_the_new_event_added() {
        let command = "/bin/mer hook --agent claude";
        let old = json!({ "hooks": {
            "SessionStart": [{ "hooks": [{ "type": "command", "command": command }] }],
            "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": command }] }],
            "Notification": [{ "hooks": [{ "type": "command", "command": command }] }],
            "Stop": [{ "hooks": [{ "type": "command", "command": command }] }],
        }});
        let (patched, changed) = patch_claude_settings(old, command).expect("patches");
        assert!(changed);
        assert_eq!(patched["hooks"]["PostToolUse"].as_array().map(Vec::len), Some(1));
        assert_eq!(patched["hooks"]["Stop"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn codex_turn_complete_is_done() {
        let payload = r#"{"type":"agent-turn-complete","thread-id":"t1"}"#;
        assert_eq!(parse_codex_payload(payload), Some((HookKind::Done, Some("t1".to_owned()))));
        assert_eq!(parse_codex_payload(r#"{"type":"other"}"#), None);
    }

    #[test]
    fn claude_patch_adds_hooks_once_and_keeps_existing_ones() {
        let existing = json!({
            "model": "opus",
            "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "echo done" }] }] }
        });
        let command = "/bin/mer hook --agent claude";

        let (patched, changed) = patch_claude_settings(existing, command).expect("patches");
        assert!(changed);
        assert_eq!(patched["model"], "opus");
        assert_eq!(patched["hooks"]["Stop"].as_array().map(Vec::len), Some(2));
        assert_eq!(patched["hooks"]["Notification"].as_array().map(Vec::len), Some(1));

        let (again, changed_again) =
            patch_claude_settings(patched.clone(), command).expect("patches");
        assert!(!changed_again);
        assert_eq!(again, patched);
    }

    #[test]
    fn claude_patch_rejects_unexpected_shapes() {
        assert!(patch_claude_settings(json!([]), "x").is_err());
        assert!(patch_claude_settings(json!({"hooks": 3}), "x").is_err());
    }

    #[test]
    fn codex_patch_keeps_comments_and_never_overwrites_a_foreign_notify() {
        let original = "# my config\nmodel = \"o3\"\n";
        let (patched, changed) = patch_codex_config(original, "/bin/mer").expect("patches");
        assert!(changed);
        assert!(patched.contains("# my config"));
        assert!(patched.contains(r#"notify = ["/bin/mer", "hook", "--agent", "codex"]"#));

        let (same, changed) = patch_codex_config(&patched, "/bin/mer").expect("idempotent");
        assert!(!changed);
        assert_eq!(same, patched);

        assert!(patch_codex_config("notify = [\"other\"]\n", "/bin/mer").is_err());
    }
}
