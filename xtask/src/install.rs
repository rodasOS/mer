//! `cargo xtask install` / `uninstall`: per-user install of the release binary.
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const ICON_SVG: &str = include_str!("../../assets/mer.svg");

/// Absolute destinations of everything the installer writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub binary: PathBuf,
    pub desktop: PathBuf,
    pub icon: PathBuf,
}

impl Layout {
    pub fn new(home: &Path) -> Self {
        Self {
            binary: home.join(".local/bin/mer"),
            desktop: home.join(".local/share/applications/mer.desktop"),
            icon: home.join(".local/share/icons/hicolor/scalable/apps/mer.svg"),
        }
    }
}

/// Builds the text of the freedesktop desktop entry.
pub fn desktop_entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=mer\n\
         GenericName=Terminal\n\
         Comment=GPU-accelerated terminal emulator for working with agents\n\
         Exec={}\n\
         Icon=mer\n\
         Terminal=false\n\
         Categories=System;TerminalEmulator;\n\
         StartupWMClass=mer\n\
         Keywords=terminal;shell;agents;\n",
        exec.display()
    )
}

/// Writes `bytes` to `dest` atomically (temp file in the same directory, then rename).
fn write_atomic(dest: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let dir = dest.parent().context("destination has no parent directory")?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = dest.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        fs::rename(&tmp, dest)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.with_context(|| format!("writing {}", dest.display()))
}

/// Installs the binary at `source`, the desktop entry and the icon under `home`.
pub fn install_files(home: &Path, source: &Path) -> Result<Layout> {
    let layout = Layout::new(home);
    let binary = fs::read(source).with_context(|| format!("reading {}", source.display()))?;
    write_atomic(&layout.binary, &binary, 0o755)?;
    write_atomic(&layout.desktop, desktop_entry(&layout.binary).as_bytes(), 0o644)?;
    write_atomic(&layout.icon, ICON_SVG.as_bytes(), 0o644)?;
    Ok(layout)
}

/// Removes the installed files; returns the ones that existed.
pub fn uninstall_files(home: &Path) -> Result<Vec<PathBuf>> {
    let layout = Layout::new(home);
    let mut removed = Vec::new();
    for path in [layout.binary, layout.desktop, layout.icon] {
        match fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {}", path.display())),
        }
    }
    Ok(removed)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set")
}

/// Runs `cargo build --release -p mer` with inherited stdio.
fn build_release(root: &Path) -> Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--release", "-p", "mer"])
        .current_dir(root)
        .status()
        .context("running cargo build")?;
    if !status.success() {
        bail!("release build failed ({status})");
    }
    Ok(())
}

/// Best-effort refresh of desktop caches; failures are only reported.
fn refresh_caches(home: &Path) {
    let apps = home.join(".local/share/applications");
    for (program, args) in [
        ("update-desktop-database", vec![apps.to_string_lossy().into_owned()]),
        ("kbuildsycoca6", Vec::new()),
    ] {
        match Command::new(program).args(&args).output() {
            Ok(out) if out.status.success() => println!("ran {program}"),
            _ => println!("note: {program} unavailable or failed (ignored)"),
        }
    }
}

pub fn install(use_pgo: bool) -> Result<()> {
    let root = crate::workspace_root();
    let home = home_dir()?;
    let source = if use_pgo {
        let binary = crate::pgo::binary(&root);
        if !binary.is_file() {
            bail!("{} does not exist; run `cargo xtask pgo` first", binary.display());
        }
        binary
    } else {
        build_release(&root)?;
        root.join("target/release/mer")
    };
    let layout = install_files(&home, &source)?;
    println!("installed binary: {}", layout.binary.display());
    println!("installed desktop entry: {}", layout.desktop.display());
    println!("installed icon: {}", layout.icon.display());
    refresh_caches(&home);
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let home = home_dir()?;
    let removed = uninstall_files(&home)?;
    if removed.is_empty() {
        println!("nothing to remove");
    }
    for path in &removed {
        println!("removed {}", path.display());
    }
    refresh_caches(&home);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `assets/mer.desktop` is what the .deb and .rpm install; it must match what `install` writes.
    #[test]
    fn packaged_desktop_file_matches_the_generated_one() {
        let packaged = include_str!("../../assets/mer.desktop");
        assert_eq!(packaged, desktop_entry(Path::new("mer")));
    }

    #[test]
    fn desktop_entry_has_required_keys() {
        let text = desktop_entry(Path::new("/home/u/.local/bin/mer"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        for line in [
            "Type=Application",
            "Name=mer",
            "Exec=/home/u/.local/bin/mer",
            "Icon=mer",
            "Terminal=false",
            "Categories=System;TerminalEmulator;",
            "StartupWMClass=mer",
            "Keywords=terminal;shell;agents;",
        ] {
            assert!(text.lines().any(|l| l == line), "missing {line}");
        }
    }

    #[test]
    fn install_and_uninstall_roundtrip() {
        let home = tempfile::tempdir().unwrap();
        let src = home.path().join("fake-mer");
        fs::write(&src, b"binary-v1").unwrap();

        let layout = install_files(home.path(), &src).unwrap();
        assert_eq!(fs::read(&layout.binary).unwrap(), b"binary-v1");
        assert_eq!(fs::metadata(&layout.binary).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(
            fs::read_to_string(&layout.desktop)
                .unwrap()
                .contains(&layout.binary.display().to_string())
        );
        assert!(fs::read_to_string(&layout.icon).unwrap().contains("<svg"));

        // Idempotent: a second install with new content replaces the binary.
        fs::write(&src, b"binary-v2").unwrap();
        install_files(home.path(), &src).unwrap();
        assert_eq!(fs::read(&layout.binary).unwrap(), b"binary-v2");
        let leftovers = fs::read_dir(layout.binary.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1);

        let removed = uninstall_files(home.path()).unwrap();
        assert_eq!(removed.len(), 3);
        assert!(!layout.binary.exists() && !layout.desktop.exists() && !layout.icon.exists());
        assert!(uninstall_files(home.path()).unwrap().is_empty());
    }
}
