//! The configuration schema.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Complete user configuration. Every field has a default so partial files work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub appearance: Appearance,
    pub terminal: Terminal,
    pub spaces: Spaces,
    pub agents: Agents,
    /// Action name (`snake_case`) to keystroke such as `ctrl-shift-t`.
    pub keys: BTreeMap<String, String>,
}

/// Which side of the window the sidebar of spaces is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarSide {
    Left,
    #[default]
    Right,
}

impl SidebarSide {
    /// Does the sidebar come before the terminals, reading left to right?
    pub fn sidebar_first(self) -> bool {
        self == Self::Left
    }

    /// The sidebar's width when its handle is at `x` pixels from the window's left edge.
    pub fn width_from_x(self, x: f32, window_width: f32) -> f32 {
        match self {
            Self::Left => x,
            Self::Right => window_width - x,
        }
    }

    /// The glyph of a button that folds the sidebar toward its window edge (`expanded`), or
    /// brings it back toward the middle (not `expanded`).
    pub fn chevron(self, expanded: bool) -> &'static str {
        match (self, expanded) {
            (Self::Left, true) | (Self::Right, false) => "«",
            (Self::Left, false) | (Self::Right, true) => "»",
        }
    }
}

/// Look and feel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: String,
    pub font_family: String,
    pub font_size: f32,
    pub line_height: f32,
    pub ui_font: String,
    /// Scale of the interface (sidebar, tabs, menus, dialogs). The terminal text has its own
    /// `font_size`. 1.0 is the default size.
    pub ui_scale: f32,
    /// Side of the window the sidebar of spaces is on.
    pub sidebar_side: SidebarSide,
}

/// Terminal behaviour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminal {
    /// `None` means auto-detect the shell.
    pub shell: Option<String>,
    pub scrollback: u32,
}

/// Space (workspace) behaviour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spaces {
    /// Minutes of inactivity before a space sleeps; 0 disables.
    pub auto_sleep_minutes: u32,
    pub restore_scrollback: bool,
    /// Closing the window leaves mer and its terminals running in the background.
    pub keep_running_on_close: bool,
}

/// Agent detection and handling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Agents {
    pub resume_on_wake: bool,
    pub notify: bool,
    /// Play a sound when an agent needs the user or finishes.
    pub sound: bool,
    /// Sound when an agent needs the user: the name of a sound of the desktop's sound theme, or
    /// the path of an audio file.
    pub sound_waiting: String,
    /// Sound when an agent finishes.
    pub sound_done: String,
    /// Volume of those sounds, 0 to 100 (percent of the player's full volume).
    pub sound_volume: u32,
    /// Alert (notification and sound) when a command that ran at least this many seconds
    /// finishes while you are not looking at it. 0 turns it off.
    pub long_command_seconds: u32,
    pub rules: Vec<AgentRuleConfig>,
}

/// A user-defined agent detection rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentRuleConfig {
    pub name: String,
    pub pattern: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            appearance: Appearance::default(),
            terminal: Terminal::default(),
            spaces: Spaces::default(),
            agents: Agents::default(),
            keys: BTreeMap::new(),
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "mocha".into(),
            font_family: "JetBrainsMono Nerd Font Mono".into(),
            font_size: 14.0,
            line_height: 1.35,
            ui_font: "Inter".into(),
            ui_scale: 1.0,
            sidebar_side: SidebarSide::default(),
        }
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self { shell: None, scrollback: 10_000 }
    }
}

impl Default for Spaces {
    fn default() -> Self {
        Self { auto_sleep_minutes: 0, restore_scrollback: true, keep_running_on_close: false }
    }
}

impl Default for Agents {
    fn default() -> Self {
        Self {
            resume_on_wake: false,
            notify: true,
            sound: true,
            sound_waiting: "message-attention".into(),
            sound_done: "complete".into(),
            sound_volume: 50,
            long_command_seconds: 30,
            rules: Vec::new(),
        }
    }
}

fn clamp_f32(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_nan() { fallback } else { v.clamp(lo, hi) }
}

impl Config {
    /// Clamp numeric values to sane ranges (font size 8..=32, line height
    /// 1.0..=2.0, interface scale 0.8..=1.6, scrollback `0..=1_000_000`). NaN falls back to the default.
    #[must_use]
    pub fn clamped(mut self) -> Config {
        let d = Appearance::default();
        self.appearance.font_size = clamp_f32(self.appearance.font_size, 8.0, 32.0, d.font_size);
        self.appearance.line_height =
            clamp_f32(self.appearance.line_height, 1.0, 2.0, d.line_height);
        self.appearance.ui_scale = clamp_f32(self.appearance.ui_scale, 0.8, 1.6, d.ui_scale);
        self.terminal.scrollback = self.terminal.scrollback.min(1_000_000);
        self.agents.sound_volume = self.agents.sound_volume.min(100);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let c = Config::default();
        assert_eq!(c.version, 1);
        assert_eq!(c.appearance.theme, "mocha");
        assert_eq!(c.appearance.font_family, "JetBrainsMono Nerd Font Mono");
        assert!((c.appearance.font_size - 14.0).abs() < f32::EPSILON);
        assert!((c.appearance.line_height - 1.35).abs() < f32::EPSILON);
        assert_eq!(c.appearance.ui_font, "Inter");
        assert!((c.appearance.ui_scale - 1.0).abs() < f32::EPSILON);
        assert_eq!(c.terminal.shell, None);
        assert_eq!(c.terminal.scrollback, 10_000);
        assert_eq!(c.spaces.auto_sleep_minutes, 0);
        assert!(c.spaces.restore_scrollback);
        assert!(!c.spaces.keep_running_on_close, "closing the window closes mer by default");
        assert!(!c.agents.resume_on_wake);
        assert!(c.agents.notify);
        assert!(c.agents.sound);
        assert_eq!(c.agents.sound_waiting, "message-attention");
        assert_eq!(c.agents.sound_done, "complete");
        assert_eq!(c.agents.sound_volume, 50);
        assert_eq!(c.agents.long_command_seconds, 30);
        assert!(c.agents.rules.is_empty());
        assert!(c.keys.is_empty());
    }

    #[test]
    fn serde_round_trip() {
        let mut c = Config::default();
        c.terminal.shell = Some("/bin/zsh".into());
        c.agents.rules.push(AgentRuleConfig { name: "x".into(), pattern: "y".into() });
        c.keys.insert("new_tab".into(), "ctrl-shift-t".into());
        let s = toml::to_string(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn clamping() {
        let mut c = Config::default();
        c.appearance.font_size = 100.0;
        c.appearance.line_height = 0.1;
        c.terminal.scrollback = u32::MAX;
        let c = c.clamped();
        assert!((c.appearance.font_size - 32.0).abs() < f32::EPSILON);
        assert!((c.appearance.line_height - 1.0).abs() < f32::EPSILON);
        assert_eq!(c.terminal.scrollback, 1_000_000);
        let mut c = Config::default();
        c.appearance.font_size = 1.0;
        c.appearance.line_height = 9.0;
        let c = c.clamped();
        assert!((c.appearance.font_size - 8.0).abs() < f32::EPSILON);
        assert!((c.appearance.line_height - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn older_files_without_sound_settings_get_the_defaults() {
        let c: Config = toml::from_str("[agents]\nnotify = false\n").unwrap();
        assert!(!c.agents.notify);
        assert!(c.agents.sound);
        assert_eq!(c.agents.sound_waiting, "message-attention");
    }

    #[test]
    fn the_sound_volume_defaults_to_half_and_never_goes_past_full() {
        let old: Config = toml::from_str("[agents]\nsound = true\n").unwrap();
        assert_eq!(old.agents.sound_volume, 50);
        let mut loud = Config::default();
        loud.agents.sound_volume = 400;
        assert_eq!(loud.clamped().agents.sound_volume, 100);
    }

    #[test]
    fn older_files_get_the_long_command_alert_at_thirty_seconds() {
        let c: Config = toml::from_str("[agents]\nnotify = true\n").unwrap();
        assert_eq!(c.agents.long_command_seconds, 30);
        let off: Config = toml::from_str("[agents]\nlong_command_seconds = 0\n").unwrap();
        assert_eq!(off.agents.long_command_seconds, 0);
    }

    #[test]
    fn the_sidebar_is_on_the_right_unless_the_file_says_otherwise() {
        assert_eq!(Config::default().appearance.sidebar_side, SidebarSide::Right);
        let old: Config = toml::from_str("[appearance]\nfont_size = 15.0\n").unwrap();
        assert_eq!(old.appearance.sidebar_side, SidebarSide::Right);
        let left: Config = toml::from_str("[appearance]\nsidebar_side = \"left\"\n").unwrap();
        assert_eq!(left.appearance.sidebar_side, SidebarSide::Left);
        let text = toml::to_string(&left).unwrap();
        assert!(text.contains("sidebar_side = \"left\""));
    }

    #[test]
    fn the_sidebar_side_decides_order_width_and_chevrons() {
        let (left, right) = (SidebarSide::Left, SidebarSide::Right);
        assert!(left.sidebar_first());
        assert!(!right.sidebar_first());
        // A handle 300 px from the left edge of a 1000 px window.
        assert!((left.width_from_x(300.0, 1000.0) - 300.0).abs() < f32::EPSILON);
        assert!((right.width_from_x(300.0, 1000.0) - 700.0).abs() < f32::EPSILON);
        // Folding points toward the nearest window edge; opening points back inwards.
        assert_eq!((left.chevron(true), left.chevron(false)), ("«", "»"));
        assert_eq!((right.chevron(true), right.chevron(false)), ("»", "«"));
    }

    #[test]
    fn the_interface_scale_is_clamped_and_older_files_get_the_default() {
        let mut c = Config::default();
        c.appearance.ui_scale = 9.0;
        assert!((c.clamped().appearance.ui_scale - 1.6).abs() < f32::EPSILON);
        let mut c = Config::default();
        c.appearance.ui_scale = f32::NAN;
        assert!((c.clamped().appearance.ui_scale - 1.0).abs() < f32::EPSILON);
        let c: Config = toml::from_str("[appearance]\nfont_size = 15.0\n").unwrap();
        assert!((c.appearance.ui_scale - 1.0).abs() < f32::EPSILON);
    }
}
