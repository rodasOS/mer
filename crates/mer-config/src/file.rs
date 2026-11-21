//! Loading and saving `config.toml`.

use std::fs;
use std::io::Write as _;
use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::config::Config;

/// Errors from [`save`].
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot serialize configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("existing config file is not valid TOML and was left untouched: {0}")]
    InvalidExisting(#[from] toml_edit::TomlError),
}

fn io_err(path: &Path, source: std::io::Error) -> ConfigError {
    ConfigError::Io { path: path.display().to_string(), source }
}

/// Result of [`load`]: always a usable config plus any problems found.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub config: Config,
    pub warnings: Vec<String>,
}

/// Load a config file. Never fails and never modifies the file: a missing
/// file yields defaults silently; unreadable or invalid files yield defaults
/// and a warning. Unknown keys are ignored.
pub fn load(path: &Path) -> Loaded {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Loaded { config: Config::default(), warnings: Vec::new() };
        }
        Err(e) => {
            return Loaded {
                config: Config::default(),
                warnings: vec![format!("cannot read {}: {e}; using defaults", path.display())],
            };
        }
    };
    match toml::from_str::<Config>(&text) {
        Ok(config) => Loaded { config, warnings: Vec::new() },
        Err(e) => Loaded {
            config: Config::default(),
            warnings: vec![format!("invalid config {}: {e}; using defaults", path.display())],
        },
    }
}

const HEADER: &str = "# mer configuration\n# Edit freely: comments and unknown keys are preserved when mer saves.\n\n";

/// f32 values are serialized through their shortest decimal form so that
/// `1.35` is written as `1.35` and not `1.350000023841858`.
fn normalize(item: &mut Item) {
    match item {
        Item::Value(v) => normalize_value(v),
        Item::Table(t) => {
            for (_, i) in t.iter_mut() {
                normalize(i);
            }
        }
        Item::ArrayOfTables(a) => {
            for t in a.iter_mut() {
                for (_, i) in t.iter_mut() {
                    normalize(i);
                }
            }
        }
        Item::None => {}
    }
}

fn normalize_value(v: &mut Value) {
    if let Value::Float(f) = v {
        #[allow(clippy::cast_possible_truncation)]
        let short = (*f.value() as f32).to_string();
        if let Ok(parsed) = short.parse::<f64>() {
            *v = Value::from(parsed);
        }
    }
}

fn numeric(v: &Value) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)]
    match v {
        Value::Float(f) => Some(*f.value()),
        Value::Integer(i) => Some(*i.value() as f64),
        _ => None,
    }
}

fn same_value(a: &Value, b: &Value) -> bool {
    if let (Some(x), Some(y)) = (numeric(a), numeric(b)) {
        return (x - y).abs() < 1e-9;
    }
    a.to_string().trim() == b.to_string().trim()
}

fn merge_item(existing: &mut Item, new: &Item) {
    match (&mut *existing, new) {
        (Item::Table(e), Item::Table(n)) => merge_table(e, n),
        (Item::Value(e), Item::Value(n)) => {
            if !same_value(e, n) {
                let decor = e.decor().clone();
                let mut nv = n.clone();
                *nv.decor_mut() = decor;
                *e = nv;
            }
        }
        (e, n) => {
            if e.to_string().trim() != n.to_string().trim() {
                *e = n.clone();
            }
        }
    }
}

fn merge_table(existing: &mut Table, new: &Table) {
    for (k, nv) in new {
        match existing.get_mut(k) {
            Some(ev) => merge_item(ev, nv),
            None => {
                existing.insert(k, nv.clone());
            }
        }
    }
}

/// Save `config` into `path`, merging into the existing document so user
/// comments, key order and unknown keys survive. Only values that differ are
/// rewritten. Parent directories are created and the write is atomic.
/// If the existing file is not valid TOML it is not touched and an error is
/// returned.
pub fn save(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let fresh_text = toml::to_string(config)?;
    let mut fresh: DocumentMut = fresh_text.parse()?;
    normalize(fresh.as_item_mut());

    let output = match fs::read_to_string(path) {
        Ok(existing_text) => {
            let mut doc: DocumentMut = existing_text.parse()?;
            merge_table(doc.as_table_mut(), fresh.as_table());
            // Drop keys the config no longer sets.
            if fresh
                .get("terminal")
                .and_then(Item::as_table)
                .is_some_and(|t| !t.contains_key("shell"))
                && let Some(t) = doc.get_mut("terminal").and_then(Item::as_table_mut)
            {
                t.remove("shell");
            }
            if let Some(Item::Table(keys)) = doc.get_mut("keys") {
                let stale: Vec<String> = keys
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .filter(|k| !config.keys.contains_key(k))
                    .collect();
                for k in stale {
                    keys.remove(&k);
                }
            }
            doc.to_string()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            format!("{HEADER}{fresh}")
        }
        Err(e) => return Err(io_err(path, e)),
    };

    write_atomic(path, &output)
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), ConfigError> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let name =
        path.file_name().map_or_else(|| "config".into(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(path, e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults_without_warning() {
        let d = tempfile::tempdir().unwrap();
        let l = load(&d.path().join("nope.toml"));
        assert_eq!(l.config, Config::default());
        assert!(l.warnings.is_empty());
    }

    #[test]
    fn partial_file_keeps_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.toml");
        fs::write(&p, "[appearance]\ntheme = \"latte\"\nunknown = 3\n[zzz]\na = 1\n").unwrap();
        let l = load(&p);
        assert!(l.warnings.is_empty());
        assert_eq!(l.config.appearance.theme, "latte");
        assert_eq!(l.config.appearance.ui_font, "Inter");
        assert_eq!(l.config.terminal.scrollback, 10_000);
    }

    #[test]
    fn invalid_toml_warns_and_leaves_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.toml");
        let bad = "this is = = not toml [";
        fs::write(&p, bad).unwrap();
        let l = load(&p);
        assert_eq!(l.config, Config::default());
        assert_eq!(l.warnings.len(), 1);
        assert_eq!(fs::read_to_string(&p).unwrap(), bad);
        assert!(save(&p, &Config::default()).is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(), bad);
    }

    #[test]
    fn save_preserves_comments_unknown_keys_and_order() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.toml");
        let original = "# my comment\nversion = 1\nmystery = \"keep\"\n\n[appearance]\n# pick a theme\ntheme = \"mocha\" # inline\nfont_size = 14\nextra = true\n\n[terminal]\nscrollback = 500\n";
        fs::write(&p, original).unwrap();
        let mut c = load(&p).config;
        assert_eq!(c.terminal.scrollback, 500);
        c.appearance.theme = "latte".into();
        save(&p, &c).unwrap();
        let out = fs::read_to_string(&p).unwrap();
        assert!(out.contains("# my comment"));
        assert!(out.contains("# pick a theme"));
        assert!(out.contains("theme = \"latte\" # inline"));
        assert!(out.contains("mystery = \"keep\""));
        assert!(out.contains("extra = true"));
        assert!(out.contains("font_size = 14\n"), "unchanged value rewritten: {out}");
        assert!(out.contains("scrollback = 500"));
        assert!(out.find("mystery").unwrap() < out.find("[appearance]").unwrap());
        assert!(out.find("theme").unwrap() < out.find("extra").unwrap());
        assert_eq!(load(&p).config, c);
    }

    #[test]
    fn save_unchanged_is_byte_identical() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.toml");
        save(&p, &Config::default()).unwrap();
        let first = fs::read_to_string(&p).unwrap();
        save(&p, &load(&p).config).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), first);
    }

    #[test]
    fn save_creates_missing_path_with_header() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/b/config.toml");
        let mut c = Config::default();
        c.terminal.shell = Some("/bin/fish".into());
        c.keys.insert("new_tab".into(), "ctrl-shift-t".into());
        save(&p, &c).unwrap();
        let out = fs::read_to_string(&p).unwrap();
        assert!(out.starts_with("# mer configuration"));
        assert!(out.contains("1.35"), "{out}");
        assert_eq!(load(&p).config, c);
    }

    #[test]
    fn none_shell_is_omitted_and_removed() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.toml");
        let mut c = Config::default();
        save(&p, &c).unwrap();
        assert!(!fs::read_to_string(&p).unwrap().contains("shell"));
        c.terminal.shell = Some("/bin/sh".into());
        save(&p, &c).unwrap();
        assert!(fs::read_to_string(&p).unwrap().contains("shell"));
        c.terminal.shell = None;
        save(&p, &c).unwrap();
        assert!(!fs::read_to_string(&p).unwrap().contains("shell"));
    }
}
