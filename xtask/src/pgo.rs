//! Profile-guided optimization: `cargo xtask pgo`, then `cargo xtask install --pgo`.
//!
//! The steps: build `mer` instrumented; run it on a real workload (a big colored `cat` plus a
//! long directory listing, drawn in a real window); merge the profiles; rebuild using them.
//! On this machine the result runs the same `cat` about 12 % faster.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Where the optimized binary ends up, relative to the workspace root.
const OUTPUT: &str = "target/pgo/use/release/mer";
const TRAINING_BYTES: usize = 32 * 1024 * 1024;
const TRAINING_RUNS: usize = 2;

/// Path of the PGO binary.
pub fn binary(root: &Path) -> PathBuf {
    root.join(OUTPUT)
}

/// The `host:` line of `rustc -vV`.
fn parse_host(rustc_version: &str) -> Option<String> {
    rustc_version.lines().find_map(|line| line.strip_prefix("host: ")).map(str::to_owned)
}

/// `llvm-profdata` from the `llvm-tools` rustup component.
fn llvm_profdata() -> Result<PathBuf> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let text = |args: &[&str]| -> Result<String> {
        let out = Command::new(&rustc).args(args).output().context("running rustc")?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let sysroot = text(&["--print", "sysroot"])?;
    let host = parse_host(&text(&["-vV"])?).context("cannot read the rustc host triple")?;
    let tool = Path::new(sysroot.trim()).join("lib/rustlib").join(host).join("bin/llvm-profdata");
    if !tool.is_file() {
        bail!("llvm-profdata not found; run `rustup component add llvm-tools` first");
    }
    Ok(tool)
}

/// Builds `mer` in `target_dir`. `RUSTFLAGS` replaces the ones from `.cargo/config.toml`, so
/// the machine-specific flag is repeated here.
fn build(root: &Path, target_dir: &Path, extra_flags: &str) -> Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--release", "-p", "mer"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target_dir)
        .env("RUSTFLAGS", format!("-C target-cpu=native {extra_flags}"))
        .status()
        .context("running cargo build")?;
    if !status.success() {
        bail!("build failed ({status})");
    }
    Ok(())
}

pub fn run() -> Result<()> {
    let root = crate::workspace_root();
    let work = root.join("target/pgo");
    let profdata = llvm_profdata()?;
    let raw = work.join("profraw");
    if raw.exists() {
        fs::remove_dir_all(&raw)?;
    }
    fs::create_dir_all(&raw)?;

    println!("== 1/4 building the instrumented binary");
    let generated = work.join("gen");
    build(&root, &generated, &format!("-C profile-generate={}", raw.display()))?;

    println!("== 2/4 training ({TRAINING_RUNS} runs)");
    let data = work.join("training.txt");
    fs::write(&data, crate::bench::generate_data(TRAINING_BYTES))?;
    let script = format!("cat {}; ls --color=always -R /usr/share | head -5000", data.display());
    for run in 1..=TRAINING_RUNS {
        // Isolated state and config: training must not touch the user's saved spaces.
        let status = Command::new("timeout")
            .args(["180"])
            .arg(generated.join("release/mer"))
            .args(["-e", "sh", "-c", &script])
            .env("XDG_STATE_HOME", work.join("state"))
            .env("XDG_CONFIG_HOME", work.join("config"))
            .status()
            .context("running the instrumented binary")?;
        if !status.success() {
            bail!("training run {run} failed ({status})");
        }
    }

    println!("== 3/4 merging profiles");
    let profiles: Vec<PathBuf> = fs::read_dir(&raw)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "profraw"))
        .collect();
    if profiles.is_empty() {
        bail!("training produced no profile data");
    }
    let merged = work.join("merged.profdata");
    let status = Command::new(profdata)
        .args(["merge", "-o"])
        .arg(&merged)
        .args(&profiles)
        .status()
        .context("running llvm-profdata")?;
    if !status.success() {
        bail!("llvm-profdata failed ({status})");
    }

    println!("== 4/4 building with the profile");
    build(
        &root,
        &work.join("use"),
        &format!("-C profile-use={} -C llvm-args=-pgo-warn-missing-function", merged.display()),
    )?;

    println!("\nPGO binary: {}", binary(&root).display());
    println!("install it with `cargo xtask install --pgo`");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_triple_is_read_from_rustc_output() {
        let text = "rustc 1.96.0 (abc 2026-05-25)\nbinary: rustc\nhost: x86_64-unknown-linux-gnu\nrelease: 1.96.0\n";
        assert_eq!(parse_host(text).as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(parse_host("nothing here"), None);
    }
}
