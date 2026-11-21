//! Application settings shared by every view: the config file, the themes and the palette.

use std::rc::Rc;

use gpui::Global;
use mer_agents::{AgentRegistry, AgentRule};
use mer_config::{Config, Theme};

use crate::theme::Palette;

/// Smallest and largest terminal text size, with zoom applied.
const MIN_FONT_SIZE: f32 = 6.0;
const MAX_FONT_SIZE: f32 = 48.0;
/// Points added or removed by one zoom step.
const ZOOM_STEP: f32 = 1.0;

/// Smallest and largest interface scale, and how much one step changes it.
const MIN_UI_SCALE: f32 = 0.8;
const MAX_UI_SCALE: f32 = 1.6;
const UI_SCALE_STEP: f32 = 0.1;

/// The interface scale after a change request. Steps of a tenth, within the allowed range.
pub fn next_ui_scale(current: f32, change: Zoom) -> f32 {
    let next = match change {
        Zoom::In => current + UI_SCALE_STEP,
        Zoom::Out => current - UI_SCALE_STEP,
        Zoom::Reset => 1.0,
    };
    ((next * 10.0).round() / 10.0).clamp(MIN_UI_SCALE, MAX_UI_SCALE)
}

/// A zoom request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

/// Lives in GPUI's global state so any view can read the current look on each frame.
pub struct AppSettings {
    pub config: Config,
    pub themes: Vec<Theme>,
    pub palette: Palette,
    /// Points added to the configured font size by zooming. Lasts for the session: it is
    /// neither saved nor shown in the settings.
    pub zoom: f32,
    /// The agents to spot (built-in plus the user's rules), built once per config change.
    pub registry: Rc<AgentRegistry>,
}

impl Global for AppSettings {}

impl AppSettings {
    pub fn new(config: Config, themes: Vec<Theme>) -> Self {
        let config = config.clamped();
        let palette =
            Palette::from_theme(mer_config::find_theme(&themes, &config.appearance.theme));
        let rules =
            config.agents.rules.iter().map(|rule| AgentRule::new(&rule.name, &rule.pattern));
        let registry = Rc::new(AgentRegistry::with_extra(rules.collect()));
        Self { config, themes, palette, zoom: 0.0, registry }
    }

    /// Reads `config.toml` and the theme files. Problems become warnings, never failures.
    pub fn load() -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let config = match mer_config::config_path() {
            Some(path) => {
                let loaded = mer_config::load(&path);
                warnings.extend(loaded.warnings);
                loaded.config
            }
            None => Config::default(),
        };
        let themes = match mer_config::themes_dir() {
            Some(dir) => {
                let (themes, theme_warnings) = mer_config::load_themes(&dir);
                warnings.extend(theme_warnings);
                themes
            }
            None => mer_config::builtin_themes(),
        };
        (Self::new(config, themes), warnings)
    }

    /// Replaces the config (clamping it) and recomputes the palette.
    pub fn set_config(&mut self, config: Config) {
        let zoom = self.zoom;
        *self = Self::new(config, std::mem::take(&mut self.themes));
        self.zoom = zoom;
    }

    /// The terminal text size in points: the configured size plus the zoom.
    pub fn font_size(&self) -> f32 {
        (self.config.appearance.font_size + self.zoom).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
    }

    /// Zooms the terminal text. The zoom stops at the smallest and largest readable sizes.
    pub fn apply_zoom(&mut self, change: Zoom) {
        let base = self.config.appearance.font_size;
        self.zoom = match change {
            Zoom::In => self.zoom + ZOOM_STEP,
            Zoom::Out => self.zoom - ZOOM_STEP,
            Zoom::Reset => 0.0,
        }
        .clamp(MIN_FONT_SIZE - base, MAX_FONT_SIZE - base);
    }

    /// Name of the active theme, falling back to the first one.
    pub fn theme_name(&self) -> &str {
        &mer_config::find_theme(&self.themes, &self.config.appearance.theme).name
    }
}

/// Writes the config file without blocking the UI.
pub fn save_in_background(config: Config) {
    let Some(path) = mer_config::config_path() else { return };
    std::thread::spawn(move || {
        if let Err(error) = mer_config::save(&path, &config) {
            tracing::warn!(%error, "could not save the config");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AppSettings {
        AppSettings::new(Config::default(), mer_config::builtin_themes())
    }

    #[test]
    fn the_interface_scale_moves_in_tenths_and_stops_at_the_limits() {
        assert!((next_ui_scale(1.0, Zoom::In) - 1.1).abs() < 1e-6);
        assert!((next_ui_scale(1.0, Zoom::Out) - 0.9).abs() < 1e-6);
        assert!((next_ui_scale(1.5, Zoom::Reset) - 1.0).abs() < 1e-6);
        assert!((next_ui_scale(1.6, Zoom::In) - 1.6).abs() < 1e-6);
        assert!((next_ui_scale(0.8, Zoom::Out) - 0.8).abs() < 1e-6);
        // Floating point drift does not pile up over many steps.
        let mut scale = 1.0;
        for _ in 0..30 {
            scale = next_ui_scale(scale, Zoom::In);
        }
        assert!((scale - 1.6).abs() < 1e-6);
    }

    #[test]
    fn zoom_adds_to_the_configured_size_and_resets() {
        let mut settings = settings();
        let base = settings.font_size();
        settings.apply_zoom(Zoom::In);
        settings.apply_zoom(Zoom::In);
        assert!((settings.font_size() - (base + 2.0 * ZOOM_STEP)).abs() < f32::EPSILON);
        settings.apply_zoom(Zoom::Out);
        assert!((settings.font_size() - (base + ZOOM_STEP)).abs() < f32::EPSILON);
        settings.apply_zoom(Zoom::Reset);
        assert!((settings.font_size() - base).abs() < f32::EPSILON);
    }

    #[test]
    fn zoom_stops_at_the_limits_without_piling_up() {
        let mut settings = settings();
        for _ in 0..200 {
            settings.apply_zoom(Zoom::In);
        }
        assert!((settings.font_size() - MAX_FONT_SIZE).abs() < f32::EPSILON);
        settings.apply_zoom(Zoom::Out);
        assert!(settings.font_size() < MAX_FONT_SIZE, "one step back already shrinks it");
        for _ in 0..200 {
            settings.apply_zoom(Zoom::Out);
        }
        assert!((settings.font_size() - MIN_FONT_SIZE).abs() < f32::EPSILON);
    }

    #[test]
    fn changing_the_config_keeps_the_zoom() {
        let mut settings = settings();
        settings.apply_zoom(Zoom::In);
        let zoom = settings.zoom;
        settings.set_config(Config::default());
        assert!((settings.zoom - zoom).abs() < f32::EPSILON);
    }
}
