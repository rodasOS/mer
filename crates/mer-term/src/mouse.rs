//! Mouse reporting: turns mouse events into the bytes terminal programs (vim, htop, less...)
//! ask for when they enable mouse tracking.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// Motion with no button held.
    None,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

/// A mouse event at a cell. `col` and `line` are zero-based, relative to the visible grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    pub button: MouseButton,
    pub action: MouseAction,
    pub col: usize,
    pub line: usize,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Highest coordinate the legacy encoding can express (a byte offset by 32 and 1).
const LEGACY_MAX: usize = 222;

/// Bytes to write to the PTY. `sgr` selects the SGR (1006) encoding, which has no coordinate
/// limit; otherwise the legacy X10 encoding is used and out-of-range cells return `None`.
pub fn encode(event: &MouseEvent, sgr: bool) -> Option<Vec<u8>> {
    let base = match event.button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::None => 3,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    let modifiers = u8::from(event.shift) * 4 + u8::from(event.alt) * 8 + u8::from(event.ctrl) * 16;
    let motion = if event.action == MouseAction::Motion { 32 } else { 0 };

    if sgr {
        let code = base + modifiers + motion;
        let suffix = if event.action == MouseAction::Release { 'm' } else { 'M' };
        return Some(
            format!("\x1b[<{code};{};{}{suffix}", event.col + 1, event.line + 1).into_bytes(),
        );
    }

    if event.col > LEGACY_MAX || event.line > LEGACY_MAX {
        return None;
    }
    // The legacy encoding reports every release as "button 3".
    let button = if event.action == MouseAction::Release { 3 } else { base };
    let code = button + modifiers + motion;
    let byte = |value: usize| u8::try_from(value + 33).ok();
    Some(vec![0x1b, b'[', b'M', code + 32, byte(event.col)?, byte(event.line)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(button: MouseButton, action: MouseAction, col: usize, line: usize) -> MouseEvent {
        MouseEvent { button, action, col, line, shift: false, alt: false, ctrl: false }
    }

    #[test]
    fn sgr_press_and_release_differ_only_in_the_final_byte() {
        let press = event(MouseButton::Left, MouseAction::Press, 0, 0);
        assert_eq!(encode(&press, true), Some(b"\x1b[<0;1;1M".to_vec()));
        let release = MouseEvent { action: MouseAction::Release, ..press };
        assert_eq!(encode(&release, true), Some(b"\x1b[<0;1;1m".to_vec()));
    }

    #[test]
    fn sgr_has_no_coordinate_limit() {
        let far = event(MouseButton::Right, MouseAction::Press, 499, 299);
        assert_eq!(encode(&far, true), Some(b"\x1b[<2;500;300M".to_vec()));
    }

    #[test]
    fn modifiers_and_motion_add_to_the_button_code() {
        let drag = MouseEvent {
            ctrl: true,
            shift: true,
            ..event(MouseButton::Left, MouseAction::Motion, 4, 2)
        };
        // 0 + shift 4 + ctrl 16 + motion 32 = 52
        assert_eq!(encode(&drag, true), Some(b"\x1b[<52;5;3M".to_vec()));
        let hover = event(MouseButton::None, MouseAction::Motion, 0, 0);
        assert_eq!(encode(&hover, true), Some(b"\x1b[<35;1;1M".to_vec()));
    }

    #[test]
    fn wheel_uses_buttons_64_and_65() {
        let up = event(MouseButton::WheelUp, MouseAction::Press, 1, 1);
        assert_eq!(encode(&up, true), Some(b"\x1b[<64;2;2M".to_vec()));
        let down = event(MouseButton::WheelDown, MouseAction::Press, 1, 1);
        assert_eq!(encode(&down, true), Some(b"\x1b[<65;2;2M".to_vec()));
    }

    #[test]
    fn legacy_encoding_offsets_every_byte_and_reports_release_as_button_three() {
        let press = event(MouseButton::Left, MouseAction::Press, 0, 0);
        assert_eq!(encode(&press, false), Some(vec![0x1b, b'[', b'M', 32, 33, 33]));
        let release = MouseEvent { action: MouseAction::Release, ..press };
        assert_eq!(encode(&release, false), Some(vec![0x1b, b'[', b'M', 35, 33, 33]));
    }

    #[test]
    fn legacy_encoding_refuses_cells_it_cannot_express() {
        let edge = event(MouseButton::Left, MouseAction::Press, 222, 222);
        assert!(encode(&edge, false).is_some());
        let beyond = event(MouseButton::Left, MouseAction::Press, 223, 0);
        assert_eq!(encode(&beyond, false), None);
        assert!(encode(&beyond, true).is_some());
    }
}
