//! Configuration schema, load/save/watch and themes.

pub mod config;
pub mod file;
pub mod paths;
pub mod theme;
pub mod watch;

pub use config::{AgentRuleConfig, Agents, Appearance, Config, SidebarSide, Spaces, Terminal};
pub use file::{ConfigError, Loaded, load, save};
pub use paths::{config_dir, config_path, themes_dir};
pub use theme::{ChromeColors, Rgb, Theme, builtin_themes, find_theme, load_themes};
pub use watch::{WatchError, WatchHandle, watch};
