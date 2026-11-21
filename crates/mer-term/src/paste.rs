//! Turning clipboard text into the bytes written to the terminal.
//!
//! Text that came from somewhere else (a web page, a chat) must not be able to act as
//! keystrokes. Bracketed paste (`ESC [ 200 ~ ... ESC [ 201 ~`) tells the program "this is
//! pasted text", but only if the text cannot contain the closing marker; otherwise everything
//! after an embedded `ESC [ 201 ~` is typed as if the user had pressed it, including `Enter`.
//! Control characters are removed in both modes for the same reason (`Ctrl+C`, `Esc`, ...).

const BEGIN: &str = "\x1b[200~";
const END: &str = "\x1b[201~";

/// `true` for characters that are kept: printable text, tab and carriage return (line breaks
/// are turned into `\r` first). Everything else in the C0 and C1 ranges, and DEL, is dropped.
fn keep(c: char) -> bool {
    matches!(c, '\t' | '\r') || !(c.is_control())
}

/// The bytes to write for pasting `text`, wrapped in bracketed-paste markers when `bracketed`.
#[must_use]
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    let clean: String = normalized.chars().filter(|&c| keep(c)).collect();
    if bracketed { format!("{BEGIN}{clean}{END}").into_bytes() } else { clean.into_bytes() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).expect("utf-8")
    }

    #[test]
    fn plain_text_is_wrapped_only_when_the_program_asked_for_it() {
        assert_eq!(text(paste_bytes("ls -la", false)), "ls -la");
        assert_eq!(text(paste_bytes("ls -la", true)), "\x1b[200~ls -la\x1b[201~");
    }

    #[test]
    fn line_breaks_become_carriage_returns() {
        assert_eq!(text(paste_bytes("a\r\nb\nc", false)), "a\rb\rc");
    }

    #[test]
    fn an_embedded_end_marker_cannot_close_the_paste_early() {
        let hostile = "echo hi\x1b[201~\rrm -rf ~\r";
        let out = text(paste_bytes(hostile, true));
        assert!(out.starts_with(BEGIN));
        assert!(out.ends_with(END));
        let inner = &out[BEGIN.len()..out.len() - END.len()];
        assert!(!inner.contains('\x1b'), "no escape may survive inside the paste: {inner:?}");
        assert!(!inner.contains(END));
    }

    #[test]
    fn control_characters_are_dropped_in_both_modes() {
        for bracketed in [false, true] {
            let out = text(paste_bytes("a\x03b\x04c\x1bd\x7fe\u{9b}f\x00g", bracketed));
            let inner = out.trim_start_matches(BEGIN).trim_end_matches(END);
            assert_eq!(inner, "abcdefg");
        }
    }

    #[test]
    fn tabs_unicode_and_emoji_are_kept() {
        assert_eq!(text(paste_bytes("col1\tcol2 ñ é 日本 🚀", false)), "col1\tcol2 ñ é 日本 🚀");
    }

    #[test]
    fn empty_text_still_produces_valid_markers() {
        assert_eq!(text(paste_bytes("", true)), "\x1b[200~\x1b[201~");
        assert!(paste_bytes("", false).is_empty());
    }
}
