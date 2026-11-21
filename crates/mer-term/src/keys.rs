//! Keyboard encoding: turns a key press into the bytes a terminal program expects.

/// A key press, independent of the UI toolkit.
#[derive(Debug, Clone, Copy)]
pub struct KeyInput<'a> {
    /// Key name: `"a"`, `"enter"`, `"up"`, `"f5"`, ...
    pub key: &'a str,
    /// Text the key would type, if any.
    pub key_char: Option<&'a str>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyInput<'_> {
    /// xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
    fn modifier_param(&self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }

    fn has_modifiers(&self) -> bool {
        self.shift || self.alt || self.ctrl
    }
}

/// Returns the bytes to write to the PTY, or `None` if the key produces nothing.
pub fn encode(input: &KeyInput, app_cursor: bool) -> Option<Vec<u8>> {
    if let Some(bytes) = encode_named(input, app_cursor) {
        return Some(bytes);
    }

    if input.ctrl
        && let Some(byte) = control_byte(input.key)
    {
        return Some(with_alt_prefix(input.alt, vec![byte]));
    }

    let text =
        input.key_char.filter(|text| !text.is_empty() && !text.chars().all(char::is_control))?;
    Some(with_alt_prefix(input.alt, text.as_bytes().to_vec()))
}

fn with_alt_prefix(alt: bool, mut bytes: Vec<u8>) -> Vec<u8> {
    if alt {
        bytes.insert(0, 0x1b);
    }
    bytes
}

fn control_byte(key: &str) -> Option<u8> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    match ch {
        'a'..='z' => Some(ch as u8 & 0x1f),
        'A'..='Z' => Some(ch.to_ascii_lowercase() as u8 & 0x1f),
        ' ' | '@' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '/' | '7' => Some(0x1f),
        _ => None,
    }
}

fn encode_named(input: &KeyInput, app_cursor: bool) -> Option<Vec<u8>> {
    let simple: Option<&[u8]> = match input.key {
        // Shift+Enter sends ESC CR (same as Alt+Enter) so agents such as Claude Code
        // can tell it apart from Enter and insert a newline instead of submitting.
        "enter" if input.shift && !input.ctrl && !input.alt => Some(b"\x1b\r"),
        "enter" => Some(b"\r"),
        "escape" => Some(b"\x1b"),
        "backspace" => Some(b"\x7f"),
        "tab" if input.shift => Some(b"\x1b[Z"),
        "tab" => Some(b"\t"),
        "space" if !input.ctrl => Some(b" "),
        _ => None,
    };
    if let Some(bytes) = simple {
        return Some(with_alt_prefix(input.alt && input.key != "escape", bytes.to_vec()));
    }

    // CSI sequences of the form ESC [ <final> (arrows, home, end).
    let csi_final = match input.key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(final_byte) = csi_final {
        let text = if input.has_modifiers() {
            format!("\x1b[1;{}{final_byte}", input.modifier_param())
        } else if app_cursor {
            format!("\x1bO{final_byte}")
        } else {
            format!("\x1b[{final_byte}")
        };
        return Some(text.into_bytes());
    }

    // SS3 keys F1-F4.
    let ss3_final = match input.key {
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    };
    if let Some(final_byte) = ss3_final {
        let text = if input.has_modifiers() {
            format!("\x1b[1;{}{final_byte}", input.modifier_param())
        } else {
            format!("\x1bO{final_byte}")
        };
        return Some(text.into_bytes());
    }

    // Tilde keys: ESC [ <code> ~.
    let tilde_code = match input.key {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    }?;
    let text = if input.has_modifiers() {
        format!("\x1b[{tilde_code};{}~", input.modifier_param())
    } else {
        format!("\x1b[{tilde_code}~")
    };
    Some(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> KeyInput<'_> {
        KeyInput { key: name, key_char: None, ctrl: false, alt: false, shift: false }
    }

    fn typed<'a>(name: &'a str, text: &'a str) -> KeyInput<'a> {
        KeyInput { key_char: Some(text), ..key(name) }
    }

    #[test]
    fn plain_text_is_sent_as_typed() {
        assert_eq!(encode(&typed("a", "a"), false), Some(b"a".to_vec()));
        assert_eq!(encode(&typed("a", "á"), false), Some("á".as_bytes().to_vec()));
    }

    #[test]
    fn ctrl_letters_become_control_bytes() {
        let input = KeyInput { ctrl: true, ..key("c") };
        assert_eq!(encode(&input, false), Some(vec![0x03]));
        let input = KeyInput { ctrl: true, ..key("l") };
        assert_eq!(encode(&input, false), Some(vec![0x0c]));
    }

    #[test]
    fn shift_enter_differs_from_enter() {
        assert_eq!(encode(&key("enter"), false), Some(b"\r".to_vec()));
        let input = KeyInput { shift: true, ..key("enter") };
        assert_eq!(encode(&input, false), Some(b"\x1b\r".to_vec()));
    }

    #[test]
    fn alt_prefixes_escape() {
        let input = KeyInput { alt: true, ..typed("b", "b") };
        assert_eq!(encode(&input, false), Some(b"\x1bb".to_vec()));
    }

    #[test]
    fn arrows_respect_application_cursor_mode() {
        assert_eq!(encode(&key("up"), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode(&key("up"), true), Some(b"\x1bOA".to_vec()));
    }

    #[test]
    fn modified_arrows_use_xterm_parameters() {
        let input = KeyInput { ctrl: true, ..key("right") };
        assert_eq!(encode(&input, false), Some(b"\x1b[1;5C".to_vec()));
        let input = KeyInput { shift: true, alt: true, ..key("left") };
        assert_eq!(encode(&input, true), Some(b"\x1b[1;4D".to_vec()));
    }

    #[test]
    fn function_and_editing_keys() {
        assert_eq!(encode(&key("f1"), false), Some(b"\x1bOP".to_vec()));
        assert_eq!(encode(&key("f5"), false), Some(b"\x1b[15~".to_vec()));
        assert_eq!(encode(&key("delete"), false), Some(b"\x1b[3~".to_vec()));
        let input = KeyInput { ctrl: true, ..key("pageup") };
        assert_eq!(encode(&input, false), Some(b"\x1b[5;5~".to_vec()));
    }

    #[test]
    fn shift_tab_is_backtab() {
        let input = KeyInput { shift: true, ..key("tab") };
        assert_eq!(encode(&input, false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(encode(&key("tab"), false), Some(b"\t".to_vec()));
    }

    #[test]
    fn bare_modifier_keys_produce_nothing() {
        assert_eq!(encode(&key("shift"), false), None);
        assert_eq!(encode(&key("control"), false), None);
    }
}
