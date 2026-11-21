//! Color themes: built-in and user-provided.

use std::fs;
use std::path::Path;

use serde::Deserialize;

/// An sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse `#rrggbb` or `rrggbb`.
    pub fn from_hex(s: &str) -> Option<Rgb> {
        let h = s.trim();
        let h = h.strip_prefix('#').unwrap_or(h);
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb::new(p(0)?, p(2)?, p(4)?))
    }

    /// Format as `#rrggbb`.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Linear blend: `t = 0.0` gives `self`, `t = 1.0` gives `other`.
    #[must_use]
    pub fn blend(self, other: Rgb, t: f32) -> Rgb {
        let m = |a: u8, b: u8| {
            let v = f32::from(a) + (f32::from(b) - f32::from(a)) * t;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let out = v.round().clamp(0.0, 255.0) as u8;
            out
        };
        Rgb::new(m(self.r, other.r), m(self.g, other.g), m(self.b, other.b))
    }
}

/// Colors of the application chrome (sidebar, bars, panels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeColors {
    pub sidebar: Rgb,
    pub bar: Rgb,
    pub surface: Rgb,
    pub border: Rgb,
    pub text: Rgb,
    pub text_dim: Rgb,
    pub accent: Rgb,
}

/// A complete theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub dark: bool,
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
    pub chrome: ChromeColors,
}

impl ChromeColors {
    /// Derive chrome colors from the terminal colors. Used for any chrome key a
    /// theme does not set:
    ///
    /// - `sidebar` and `bar`: background blended 25% toward black (dark
    ///   themes) or 40% toward white (light themes);
    /// - `surface`: background blended 10% toward the foreground;
    /// - `border`: background blended 18% toward the foreground;
    /// - `text`: the foreground;
    /// - `text_dim`: background blended 50% toward the foreground;
    /// - `accent`: `ansi[4]` (blue).
    #[must_use]
    pub fn derive(dark: bool, foreground: Rgb, background: Rgb, ansi: &[Rgb; 16]) -> Self {
        let side = if dark {
            background.blend(Rgb::new(0, 0, 0), 0.25)
        } else {
            background.blend(Rgb::new(255, 255, 255), 0.4)
        };
        Self {
            sidebar: side,
            bar: side,
            surface: background.blend(foreground, 0.10),
            border: background.blend(foreground, 0.18),
            text: foreground,
            text_dim: background.blend(foreground, 0.5),
            accent: ansi[4],
        }
    }
}

fn hexes<const N: usize>(list: [&str; N]) -> [Rgb; N] {
    list.map(|s| Rgb::from_hex(s).expect("valid built-in color"))
}

fn rgb(s: &str) -> Rgb {
    Rgb::from_hex(s).expect("valid built-in color")
}

fn derived(name: &str, dark: bool, fg: &str, bg: &str, cursor: &str, ansi: [&str; 16]) -> Theme {
    let ansi = hexes(ansi);
    let (foreground, background) = (rgb(fg), rgb(bg));
    Theme {
        name: name.into(),
        dark,
        foreground,
        background,
        cursor: rgb(cursor),
        chrome: ChromeColors::derive(dark, foreground, background, &ansi),
        ansi,
    }
}

/// The built-in themes: `mocha`, `tokyo-night`, `gruvbox-dark`, `latte`.
pub fn builtin_themes() -> Vec<Theme> {
    let mocha = Theme {
        name: "mocha".into(),
        dark: true,
        foreground: rgb("cdd6f4"),
        background: rgb("1e1e2e"),
        cursor: rgb("f5e0dc"),
        ansi: hexes([
            "45475a", "f38ba8", "a6e3a1", "f9e2af", "89b4fa", "f5c2e7", "94e2d5", "bac2de",
            "585b70", "f38ba8", "a6e3a1", "f9e2af", "89b4fa", "f5c2e7", "94e2d5", "a6adc8",
        ]),
        chrome: ChromeColors {
            sidebar: rgb("181825"),
            bar: rgb("181825"),
            surface: rgb("313244"),
            border: rgb("313244"),
            text: rgb("cdd6f4"),
            text_dim: rgb("6c7086"),
            accent: rgb("89b4fa"),
        },
    };
    vec![
        mocha,
        derived(
            "tokyo-night",
            true,
            "c0caf5",
            "1a1b26",
            "c0caf5",
            [
                "15161e", "f7768e", "9ece6a", "e0af68", "7aa2f7", "bb9af7", "7dcfff", "a9b1d6",
                "414868", "f7768e", "9ece6a", "e0af68", "7aa2f7", "bb9af7", "7dcfff", "c0caf5",
            ],
        ),
        derived(
            "gruvbox-dark",
            true,
            "ebdbb2",
            "282828",
            "ebdbb2",
            [
                "282828", "cc241d", "98971a", "d79921", "458588", "b16286", "689d6a", "a89984",
                "928374", "fb4934", "b8bb26", "fabd2f", "83a598", "d3869b", "8ec07c", "ebdbb2",
            ],
        ),
        derived(
            "latte",
            false,
            "4c4f69",
            "eff1f5",
            "dc8a78",
            [
                "5c5f77", "d20f39", "40a02b", "df8e1d", "1e66f5", "ea76cb", "179299", "acb0be",
                "6c6f85", "d20f39", "40a02b", "df8e1d", "1e66f5", "ea76cb", "179299", "bcc0cc",
            ],
        ),
    ]
}

#[derive(Deserialize)]
struct ThemeFile {
    name: Option<String>,
    dark: Option<bool>,
    colors: ColorsFile,
    chrome: Option<ChromeFile>,
}

#[derive(Deserialize)]
struct ColorsFile {
    foreground: String,
    background: String,
    cursor: Option<String>,
    ansi: Vec<String>,
}

#[derive(Deserialize, Default)]
struct ChromeFile {
    sidebar: Option<String>,
    bar: Option<String>,
    surface: Option<String>,
    border: Option<String>,
    text: Option<String>,
    text_dim: Option<String>,
    accent: Option<String>,
}

fn parse_color(label: &str, s: &str) -> Result<Rgb, String> {
    Rgb::from_hex(s).ok_or_else(|| format!("invalid color {s:?} for {label}"))
}

fn opt_color(label: &str, s: Option<&String>) -> Result<Option<Rgb>, String> {
    s.map(|v| parse_color(label, v)).transpose()
}

fn parse_theme(text: &str, stem: &str) -> Result<Theme, String> {
    let f: ThemeFile = toml::from_str(text).map_err(|e| e.to_string())?;
    if f.colors.ansi.len() != 16 {
        return Err(format!("colors.ansi must have 16 entries, found {}", f.colors.ansi.len()));
    }
    let foreground = parse_color("colors.foreground", &f.colors.foreground)?;
    let background = parse_color("colors.background", &f.colors.background)?;
    let cursor = opt_color("colors.cursor", f.colors.cursor.as_ref())?.unwrap_or(foreground);
    let mut ansi = [Rgb::new(0, 0, 0); 16];
    for (slot, s) in ansi.iter_mut().zip(&f.colors.ansi) {
        *slot = parse_color("colors.ansi", s)?;
    }
    let dark = f.dark.unwrap_or(true);
    let d = ChromeColors::derive(dark, foreground, background, &ansi);
    let c = f.chrome.unwrap_or_default();
    let chrome = ChromeColors {
        sidebar: opt_color("chrome.sidebar", c.sidebar.as_ref())?.unwrap_or(d.sidebar),
        bar: opt_color("chrome.bar", c.bar.as_ref())?.unwrap_or(d.bar),
        surface: opt_color("chrome.surface", c.surface.as_ref())?.unwrap_or(d.surface),
        border: opt_color("chrome.border", c.border.as_ref())?.unwrap_or(d.border),
        text: opt_color("chrome.text", c.text.as_ref())?.unwrap_or(d.text),
        text_dim: opt_color("chrome.text_dim", c.text_dim.as_ref())?.unwrap_or(d.text_dim),
        accent: opt_color("chrome.accent", c.accent.as_ref())?.unwrap_or(d.accent),
    };
    Ok(Theme {
        name: f.name.unwrap_or_else(|| stem.to_string()),
        dark,
        foreground,
        background,
        cursor,
        ansi,
        chrome,
    })
}

/// Load built-in themes followed by valid user themes from `dir`. A user theme
/// with the same name replaces the built-in one. Bad files are skipped with a
/// warning; a missing directory is fine. A missing `cursor` defaults to the
/// foreground color.
pub fn load_themes(dir: &Path) -> (Vec<Theme>, Vec<String>) {
    let mut themes = builtin_themes();
    let mut warnings = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return (themes, warnings);
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    for path in files {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let parsed = fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| parse_theme(&t, &stem));
        match parsed {
            Ok(theme) => {
                if let Some(slot) = themes.iter_mut().find(|t| t.name == theme.name) {
                    *slot = theme;
                } else {
                    themes.push(theme);
                }
            }
            Err(e) => warnings.push(format!("skipping theme {}: {e}", path.display())),
        }
    }
    (themes, warnings)
}

/// Find a theme by name, falling back to the first one. Panics only if
/// `themes` is empty.
pub fn find_theme<'a>(themes: &'a [Theme], name: &str) -> &'a Theme {
    themes.iter().find(|t| t.name == name).unwrap_or(&themes[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(Rgb::from_hex("#1e1e2e"), Some(Rgb::new(0x1e, 0x1e, 0x2e)));
        assert_eq!(Rgb::from_hex("FFFFFF"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(Rgb::from_hex("#fff"), None);
        assert_eq!(Rgb::from_hex("#gggggg"), None);
        assert_eq!(Rgb::from_hex(""), None);
        assert_eq!(Rgb::from_hex("#é1234"), None);
        assert_eq!(Rgb::new(1, 2, 255).to_hex(), "#0102ff");
    }

    #[test]
    fn builtins_are_sane() {
        let themes = builtin_themes();
        let names: Vec<_> = themes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["mocha", "tokyo-night", "gruvbox-dark", "latte"]);
        for t in &themes {
            let mut a: Vec<_> = t.ansi.to_vec();
            a.sort_by_key(|c| (c.r, c.g, c.b));
            a.dedup();
            assert!(a.len() >= 9, "{} has too few distinct colors", t.name);
        }
        assert!(!themes[3].dark);
    }

    #[test]
    fn mocha_matches_spec() {
        let t = &builtin_themes()[0];
        assert_eq!(t.background.to_hex(), "#1e1e2e");
        assert_eq!(t.foreground.to_hex(), "#cdd6f4");
        assert_eq!(t.cursor.to_hex(), "#f5e0dc");
        let ansi: Vec<_> = t.ansi.iter().map(|c| c.to_hex()[1..].to_string()).collect();
        assert_eq!(
            ansi.join(" "),
            "45475a f38ba8 a6e3a1 f9e2af 89b4fa f5c2e7 94e2d5 bac2de 585b70 f38ba8 a6e3a1 f9e2af 89b4fa f5c2e7 94e2d5 a6adc8"
        );
        assert_eq!(t.chrome.sidebar.to_hex(), "#181825");
        assert_eq!(t.chrome.bar.to_hex(), "#181825");
        assert_eq!(t.chrome.surface.to_hex(), "#313244");
        assert_eq!(t.chrome.border.to_hex(), "#313244");
        assert_eq!(t.chrome.text.to_hex(), "#cdd6f4");
        assert_eq!(t.chrome.text_dim.to_hex(), "#6c7086");
        assert_eq!(t.chrome.accent.to_hex(), "#89b4fa");
    }

    fn theme_toml(name_line: &str, chrome: &str) -> String {
        let ansi: Vec<String> = (0..16).map(|i| format!("\"#{i:02x}{i:02x}{i:02x}\"")).collect();
        format!(
            "{name_line}\n[colors]\nforeground = \"#ffffff\"\nbackground = \"#000000\"\nansi = [{}]\n{chrome}",
            ansi.join(", ")
        )
    }

    #[test]
    fn user_theme_overrides_builtin_and_adds_new() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a.toml"), theme_toml("name = \"mocha\"", "")).unwrap();
        fs::write(d.path().join("my-theme.toml"), theme_toml("", "")).unwrap();
        let (themes, warnings) = load_themes(d.path());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(themes.len(), 5);
        assert_eq!(find_theme(&themes, "mocha").background.to_hex(), "#000000");
        assert_eq!(find_theme(&themes, "my-theme").name, "my-theme");
        assert_eq!(themes[0].name, "mocha");
    }

    #[test]
    fn bad_theme_warns_and_missing_dir_ok() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("bad.toml"), "name = \"x\"\n[colors]\nforeground = \"nope\"\n")
            .unwrap();
        fs::write(
            d.path().join("short.toml"),
            "[colors]\nforeground=\"#fff000\"\nbackground=\"#000000\"\nansi=[\"#000000\"]\n",
        )
        .unwrap();
        fs::write(d.path().join("readme.txt"), "ignored").unwrap();
        let (themes, warnings) = load_themes(d.path());
        assert_eq!(themes.len(), 4);
        assert_eq!(warnings.len(), 2);
        let (themes, warnings) = load_themes(&d.path().join("missing"));
        assert_eq!(themes.len(), 4);
        assert!(warnings.is_empty());
    }

    #[test]
    fn chrome_is_derived_when_absent_and_overridable() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("t.toml"), theme_toml("dark = true", "")).unwrap();
        fs::write(d.path().join("u.toml"), theme_toml("", "[chrome]\naccent = \"#ff0000\"\n"))
            .unwrap();
        let (themes, w) = load_themes(d.path());
        assert!(w.is_empty(), "{w:?}");
        let t = find_theme(&themes, "t");
        assert_eq!(t.chrome.text, t.foreground);
        assert_eq!(t.chrome.accent, t.ansi[4]);
        assert_eq!(t.chrome.sidebar, Rgb::new(0, 0, 0));
        assert_eq!(t.chrome.surface, Rgb::new(26, 26, 26));
        assert_eq!(t.chrome.text_dim, Rgb::new(128, 128, 128));
        assert_eq!(find_theme(&themes, "u").chrome.accent.to_hex(), "#ff0000");
        assert_eq!(find_theme(&themes, "nonexistent").name, "mocha");
    }
}
